//! Currency conversion.
//!
//! Provider rates are stored raw and never mutated. Everything the user can
//! tweak - a percentage margin, an absolute shading, or a hard manual rate -
//! lives in `fx_setting` and is applied here, on read. That keeps the feed
//! auditable and makes "why is this number what it is?" answerable.
//!
//! A pair set to `WISE` has no fixed margin: what Wise charges depends on the
//! amount, so every conversion asks for the rate *for its amount* - a small
//! balance pays a higher percentage than a large one.

use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

use chrono::NaiveDate;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};

use crate::entities::{fx_fee, fx_rate, fx_setting};
use crate::error::AppResult;
use crate::types::MarginMode;

use super::money::{round_money, ConvertedMoney};

/// Currencies tried as a bridge when no direct or inverse rate exists.
const PIVOTS: &[&str] = &["USD", "EUR", "SGD"];

/// Every headline figure is also shown in these, after the viewer's own
/// base currency - see [`FxEngine::display_currencies`].
pub const DISPLAY_CURRENCIES: &[&str] = &["INR", "USD", "SGD"];

/// What one unit of a currency is worth in a display currency.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DisplayRate {
    pub currency: String,
    /// `None` when no path to the currency exists - shown as a gap, never 1:1.
    pub rate: Option<f64>,
    pub source: Option<String>,
    pub adjusted: bool,
}

/// One figure summed in every display currency at once, each currency's
/// part converted from its own currency rather than via the base - so a
/// Singapore deposit counts at face value in SGD. The parts are summed per
/// currency first and each converted as one amount, so a transfer fee is
/// worked out on the whole of it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Equivalents(BTreeMap<String, f64>);

impl Equivalents {
    pub fn add(&mut self, amount: f64, from: &str) {
        if amount == 0.0 {
            return;
        }
        *self.0.entry(from.to_uppercase()).or_insert(0.0) += amount;
    }

    /// A currency with no rate for any part is `None`: a total missing a
    /// part is not a total.
    pub fn rounded(&self, fx: &FxEngine) -> BTreeMap<String, Option<f64>> {
        fx.display_currencies()
            .into_iter()
            .map(|to| {
                let sum = self.0.iter().try_fold(0.0, |sum, (from, amount)| {
                    fx.convert(*amount, from, &to).map(|c| sum + c.base_amount)
                });
                (to, sum.map(round_money))
            })
            .collect()
    }
}

/// One rate per currency for a page's totals - the rate every holding in it
/// is valued at (no fee by amount, see `FxEngine::convert`), so rows, pages
/// and the dashboard all agree. A currency with no rate is flagged and
/// counted at 1:1, as `FxEngine::to_base_or_flag` does.
#[derive(Clone, Debug, Default)]
pub struct TotalRates(HashMap<String, ConvertedMoney>);

impl TotalRates {
    pub fn new<'a>(fx: &FxEngine, amounts: impl IntoIterator<Item = (&'a str, f64)>) -> Self {
        let mut sums: HashMap<String, f64> = HashMap::new();
        for (currency, amount) in amounts {
            *sums.entry(currency.to_uppercase()).or_insert(0.0) += amount;
        }
        Self(sums.into_iter().map(|(c, sum)| {
            let converted = fx.to_base_or_flag(sum, &c);
            (c, converted)
        }).collect())
    }

    /// Units of base per unit of `currency` for this page's totals.
    pub fn rate(&self, currency: &str) -> f64 {
        self.get(currency).map(|c| c.fx_rate).unwrap_or(1.0)
    }

    pub fn get(&self, currency: &str) -> Option<&ConvertedMoney> {
        self.0.get(&currency.to_uppercase())
    }
}

/// A page's figures in every display currency, priced like `TotalRates`:
/// for each display currency, one rate per source currency from converting
/// the page's whole amount in it. Slices of the page - a bank, a segment -
/// use the same rates, so each column adds up to the page's total.
#[derive(Clone, Debug, Default)]
pub struct DisplayTotals {
    /// display currency -> source currency -> rate, `None` with no path.
    rates: BTreeMap<String, HashMap<String, Option<f64>>>,
}

impl DisplayTotals {
    pub fn new<'a>(fx: &FxEngine, amounts: impl IntoIterator<Item = (&'a str, f64)>) -> Self {
        let mut sums: HashMap<String, f64> = HashMap::new();
        for (currency, amount) in amounts {
            *sums.entry(currency.to_uppercase()).or_insert(0.0) += amount;
        }
        let rates = fx
            .display_currencies()
            .into_iter()
            .map(|to| {
                let per_source = sums
                    .iter()
                    .map(|(from, sum)| (from.clone(), fx.convert(*sum, from, &to).map(|c| c.fx_rate)))
                    .collect();
                (to, per_source)
            })
            .collect();
        Self { rates }
    }

    /// `parts` - amounts in their own currencies, all among the page's -
    /// in each display currency. `None` where some part has no rate.
    pub fn values<'a>(&self, parts: impl IntoIterator<Item = (&'a str, f64)> + Clone) -> BTreeMap<String, Option<f64>> {
        self.rates
            .iter()
            .map(|(to, per_source)| {
                let sum = parts.clone().into_iter().try_fold(0.0, |sum, (from, amount)| {
                    if amount == 0.0 {
                        return Some(sum);
                    }
                    let rate = (*per_source.get(&from.to_uppercase())?)?;
                    Some(sum + amount * rate)
                });
                (to.clone(), sum.map(round_money))
            })
            .collect()
    }
}

/// What a transfer service charges to convert, by amount: the fee, in the
/// source currency, at a handful of sampled amounts.
///
/// Wise charges each slice above a volume threshold at a lower percentage,
/// so its total fee is piecewise linear in the amount. Interpolating between
/// samples taken near the corners reproduces it closely; past the last
/// sample the last slice's rate carries on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FeeSchedule {
    /// `(amount, fee)`, ascending by amount, amounts positive.
    points: Vec<(f64, f64)>,
}

impl FeeSchedule {
    pub fn new(points: impl IntoIterator<Item = (f64, f64)>) -> Option<Self> {
        let mut points: Vec<(f64, f64)> = points
            .into_iter()
            .filter(|(a, f)| a.is_finite() && f.is_finite() && *a > 0.0 && *f >= 0.0)
            .collect();
        points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        points.dedup_by(|a, b| (a.0 - b.0).abs() < f64::EPSILON);
        (!points.is_empty()).then_some(Self { points })
    }

    pub fn points(&self) -> &[(f64, f64)] {
        &self.points
    }

    /// The fee on converting `amount`, in the same currency. The sign of the
    /// amount is ignored: a fee is a cost either way.
    pub fn fee(&self, amount: f64) -> f64 {
        let a = amount.abs();
        let mut prev = (0.0, 0.0);
        for &(x, f) in &self.points {
            if a <= x {
                return prev.1 + (f - prev.1) * (a - prev.0) / (x - prev.0);
            }
            prev = (x, f);
        }
        let n = self.points.len();
        let (x0, f0) = if n >= 2 { self.points[n - 2] } else { (0.0, 0.0) };
        let slope = (prev.1 - f0) / (prev.0 - x0);
        prev.1 + slope * (a - prev.0)
    }

    /// The fee as a share of the amount, never more than all of it. At zero,
    /// the first slice's rate: what a very small amount would pay.
    pub fn fraction(&self, amount: f64) -> f64 {
        let a = amount.abs();
        let share = if a < 1e-9 {
            let (x, f) = self.points[0];
            f / x
        } else {
            self.fee(a) / a
        };
        share.clamp(0.0, 1.0)
    }
}

/// A conversion's rate once any amount-based fee is taken off.
#[derive(Clone, Debug)]
pub struct AmountQuote {
    pub quote: RateQuote,
    /// Units of `to` actually received per unit of `from`, for this amount.
    pub rate: f64,
    /// The fee as a percentage of the amount, where a fee schedule applies.
    pub fee_pct: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct RateQuote {
    pub from: String,
    pub to: String,
    /// Straight from the provider, before any user adjustment.
    pub raw_rate: f64,
    /// What the app actually converts with.
    pub effective_rate: f64,
    pub margin_mode: String,
    pub margin_value: f64,
    pub use_manual: bool,
    pub source: String,
    pub as_of: Option<NaiveDate>,
}

impl RateQuote {
    pub fn adjusted(&self) -> bool {
        (self.effective_rate - self.raw_rate).abs() > f64::EPSILON
    }
}

#[derive(Clone, Copy, Debug)]
struct RawRate {
    rate: f64,
    as_of: NaiveDate,
}

/// An immutable snapshot of rates + settings, built once per request.
///
/// Building it costs two queries; conversions afterwards are pure map lookups,
/// so a thousand-row holdings table does not hammer the database.
#[derive(Clone, Debug)]
pub struct FxEngine {
    base: String,
    /// Latest raw rate per (from, to), keyed uppercase.
    rates: HashMap<(String, String), RawRate>,
    settings: HashMap<(String, String), fx_setting::Model>,
    /// Latest fee schedule per (from, to), for pairs set to `WISE`.
    fees: HashMap<(String, String), FeeSchedule>,
}

impl FxEngine {
    /// Rates as fetched, with `owner`'s margins and manual rates applied.
    /// With no owner, the raw feed alone - what the market-data refresh wants.
    pub async fn load(db: &DatabaseConnection, base: &str, owner: Option<i32>) -> AppResult<Self> {
        // Ascending by date so later rows overwrite earlier ones and the map
        // ends up holding the most recent rate per pair.
        let rows = fx_rate::Entity::find()
            .order_by_asc(fx_rate::Column::AsOf)
            .order_by_asc(fx_rate::Column::Id)
            .all(db)
            .await?;

        let mut rates = HashMap::new();
        for r in rows {
            rates.insert(
                (r.base_code.to_uppercase(), r.quote_code.to_uppercase()),
                RawRate { rate: r.rate, as_of: r.as_of },
            );
        }

        let settings = match owner {
            Some(user) => fx_setting::Entity::find().filter(fx_setting::Column::UserId.eq(user)).all(db).await?,
            None => Vec::new(),
        }
            .into_iter()
            .map(|s| ((s.base_code.to_uppercase(), s.quote_code.to_uppercase()), s))
            .collect();

        let fees = fx_fee::Entity::find()
            .all(db)
            .await?
            .into_iter()
            .filter_map(|f| {
                let points: Vec<(f64, f64)> = serde_json::from_str(&f.points).ok()?;
                let schedule = FeeSchedule::new(points)?;
                Some(((f.base_code.to_uppercase(), f.quote_code.to_uppercase()), schedule))
            })
            .collect();

        Ok(Self { base: base.to_uppercase(), rates, settings, fees })
    }

    /// An engine with no rates - every non-base conversion will fail loudly
    /// rather than silently pretending 1:1.
    pub fn empty(base: &str) -> Self {
        Self {
            base: base.to_uppercase(),
            rates: HashMap::new(),
            settings: HashMap::new(),
            fees: HashMap::new(),
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// The currencies every headline figure is shown in: the base first -
    /// the viewer's main currency - then the usual others.
    pub fn display_currencies(&self) -> Vec<String> {
        let mut out = vec![self.base.clone()];
        out.extend(DISPLAY_CURRENCIES.iter().map(|c| c.to_string()).filter(|c| *c != self.base));
        out
    }

    /// Latest available raw rate for a pair, direct only.
    fn direct(&self, from: &str, to: &str) -> Option<(f64, String, NaiveDate)> {
        self.rates
            .get(&(from.to_string(), to.to_string()))
            .map(|r| (r.rate, "provider".to_string(), r.as_of))
    }

    /// Provider rates are stored one way round; the reverse is just 1/x.
    fn inverse(&self, from: &str, to: &str) -> Option<(f64, String, NaiveDate)> {
        self.rates
            .get(&(to.to_string(), from.to_string()))
            .filter(|r| r.rate.abs() > f64::EPSILON)
            .map(|r| (1.0 / r.rate, "inverse".to_string(), r.as_of))
    }

    /// Bridge through a pivot currency, e.g. AUD -> USD -> INR: the usual
    /// pivots and the base first, then any currency the feed has rates
    /// against. Rates are stored against the owner's main currency, so a
    /// view in another one (SGD -> USD, for a USD page) bridges through it.
    fn cross(&self, from: &str, to: &str) -> Option<(f64, String, NaiveDate)> {
        let mut stored: Vec<&str> = self.rates.keys().flat_map(|(a, b)| [a.as_str(), b.as_str()]).collect();
        stored.sort_unstable();
        stored.dedup();
        let preferred = PIVOTS.iter().copied().chain(std::iter::once(self.base.as_str()));
        let mut tried: Vec<&str> = Vec::new();
        for pivot in preferred.chain(stored) {
            if tried.contains(&pivot) {
                continue;
            }
            tried.push(pivot);
            if pivot == from || pivot == to {
                continue;
            }
            let Some(leg1) = self.direct(from, pivot).or_else(|| self.inverse(from, pivot)) else {
                continue;
            };
            let Some(leg2) = self.direct(pivot, to).or_else(|| self.inverse(pivot, to)) else {
                continue;
            };
            // The cross is only as fresh as its stalest leg.
            let as_of = leg1.2.min(leg2.2);
            return Some((leg1.0 * leg2.0, format!("cross:{pivot}"), as_of));
        }
        None
    }

    /// Resolve `from` -> `to` including any user margin or manual override.
    ///
    /// Margins are applied once, from the `(from, to)` setting only. A crossed
    /// rate does not also pick up the margins of its legs - one conversion,
    /// one spread.
    pub fn quote(&self, from: &str, to: &str) -> Option<RateQuote> {
        let from = from.to_uppercase();
        let to = to.to_uppercase();
        let setting = self.settings.get(&(from.clone(), to.clone()));

        if from == to {
            return Some(RateQuote {
                from,
                to,
                raw_rate: 1.0,
                effective_rate: 1.0,
                margin_mode: MarginMode::None.to_string(),
                margin_value: 0.0,
                use_manual: false,
                source: "identity".to_string(),
                as_of: None,
            });
        }

        // A manual override short-circuits the feed entirely - it does not
        // then get a margin applied on top, which would be a double nudge.
        if let Some(s) = setting {
            if s.use_manual {
                if let Some(manual) = s.manual_rate {
                    return Some(RateQuote {
                        from,
                        to,
                        raw_rate: manual,
                        effective_rate: manual,
                        margin_mode: s.margin_mode.clone(),
                        margin_value: s.margin_value,
                        use_manual: true,
                        source: "manual".to_string(),
                        as_of: None,
                    });
                }
            }
        }

        let (raw, how, as_of) = self
            .direct(&from, &to)
            .or_else(|| self.inverse(&from, &to))
            .or_else(|| self.cross(&from, &to))?;

        let (mode, value) = setting
            .map(|s| (s.margin_mode.clone(), s.margin_value))
            .unwrap_or_else(|| (MarginMode::None.to_string(), 0.0));

        let effective = apply_margin(raw, &mode, value);

        Some(RateQuote {
            from,
            to,
            raw_rate: raw,
            effective_rate: effective,
            margin_mode: mode,
            margin_value: value,
            use_manual: false,
            source: how,
            as_of: Some(as_of),
        })
    }

    /// Convert into the portfolio base currency, carrying the rate along.
    ///
    /// Returns `None` when no path exists - callers surface that as an
    /// "unconverted" badge rather than quietly treating it as base currency.
    pub fn to_base(&self, amount: f64, currency: &str) -> Option<ConvertedMoney> {
        self.convert(amount, currency, &self.base.clone())
    }

    /// Value `amount` at the pair's effective rate - margin or manual rate
    /// included, no fee charged by amount. Every holding, page and total is
    /// valued this way, so one currency has one rate and every figure adds
    /// up whichever page reads it. A fee is a cost of actually converting:
    /// see [`Self::convert_with_fee`].
    pub fn convert(&self, amount: f64, from: &str, to: &str) -> Option<ConvertedMoney> {
        let q = self.quote(from, to)?;
        Some(ConvertedMoney {
            amount,
            currency: q.from.clone(),
            fx_rate: q.effective_rate,
            base_amount: amount * q.effective_rate,
            base_currency: q.to.clone(),
            fx_source: q.source.clone(),
            fx_as_of: q.as_of,
            fx_adjusted: q.use_manual || q.adjusted(),
            fx_fee_pct: None,
        })
    }

    /// What converting `amount` would actually give: the effective rate less
    /// the fee a `WISE` pair's schedule charges on that amount. For the
    /// converter, not for valuing holdings - see [`Self::convert`].
    pub fn convert_with_fee(&self, amount: f64, from: &str, to: &str) -> Option<ConvertedMoney> {
        let q = self.quote_for(amount, from, to)?;
        Some(ConvertedMoney {
            amount,
            currency: q.quote.from.clone(),
            fx_rate: q.rate,
            base_amount: amount * q.rate,
            base_currency: q.quote.to.clone(),
            fx_source: q.quote.source.clone(),
            fx_as_of: q.quote.as_of,
            fx_adjusted: q.quote.use_manual || q.quote.adjusted() || q.fee_pct.is_some(),
            fx_fee_pct: q.fee_pct,
        })
    }

    /// The rate for converting `amount`: the quote, less the fee a `WISE`
    /// pair's schedule charges on that amount. Without a schedule - not
    /// fetched yet, or a manual rate - it is the quote as it stands.
    pub fn quote_for(&self, amount: f64, from: &str, to: &str) -> Option<AmountQuote> {
        let quote = self.quote(from, to)?;
        let fee = self.fee_schedule(&quote).map(|s| s.fraction(amount));
        Some(AmountQuote {
            rate: quote.effective_rate * (1.0 - fee.unwrap_or(0.0)),
            fee_pct: fee.map(|f| f * 100.0),
            quote,
        })
    }

    /// The fee schedule behind a quote, if its pair is set to `WISE`. Only
    /// the pair's own direction: a crossed or inverted rate borrows no fee,
    /// as it borrows no margin.
    fn fee_schedule(&self, q: &RateQuote) -> Option<&FeeSchedule> {
        if q.use_manual || MarginMode::from_str(&q.margin_mode) != Ok(MarginMode::Wise) {
            return None;
        }
        self.fees.get(&(q.from.clone(), q.to.clone()))
    }

    /// The fee schedule stored for a pair, whatever its adjustment - for the
    /// Forex tab.
    pub fn stored_fee_schedule(&self, from: &str, to: &str) -> Option<&FeeSchedule> {
        self.fees.get(&(from.to_uppercase(), to.to_uppercase()))
    }

    /// Conversion that degrades to 1:1 and says so, for totals that must still
    /// render when one obscure currency has no feed.
    pub fn to_base_or_flag(&self, amount: f64, currency: &str) -> ConvertedMoney {
        self.to_base(amount, currency).unwrap_or(ConvertedMoney {
            amount,
            currency: currency.to_uppercase(),
            fx_rate: 1.0,
            base_amount: amount,
            base_currency: self.base.clone(),
            fx_source: "unavailable".to_string(),
            fx_as_of: None,
            fx_adjusted: false,
            fx_fee_pct: None,
        })
    }

    /// One unit of `from` in each display currency, at the rate holdings are
    /// valued at (see [`Self::convert`]). `_amount` is kept for callers; the
    /// rate no longer depends on it.
    pub fn display_rates(&self, from: &str, _amount: f64) -> Vec<DisplayRate> {
        self.display_currencies()
            .iter()
            .map(|to| match self.quote(from, to) {
                Some(q) => DisplayRate {
                    currency: to.to_string(),
                    rate: Some(q.effective_rate),
                    adjusted: q.use_manual || q.adjusted(),
                    source: Some(q.source),
                },
                None => DisplayRate { currency: to.to_string(), rate: None, source: None, adjusted: false },
            })
            .collect()
    }

    /// Every pair the engine can currently price, for the Forex tab.
    pub fn known_pairs(&self) -> Vec<(String, String)> {
        let mut pairs: Vec<(String, String)> =
            self.settings.keys().cloned().chain(self.rates.keys().cloned()).collect();
        pairs.sort();
        pairs.dedup();
        pairs
    }
}

/// `PERCENT` shifts by a proportion, `ABSOLUTE` by quote-currency units.
/// Negative values shade the rate down - the usual case, since a bank fill is
/// worse than the mid-market rate a feed publishes.
pub fn apply_margin(raw: f64, mode: &str, value: f64) -> f64 {
    match MarginMode::from_str(mode).unwrap_or(MarginMode::None) {
        // Wise charges by amount - see `FxEngine::quote_for`.
        MarginMode::None | MarginMode::Wise => raw,
        MarginMode::Percent => raw * (1.0 + value / 100.0),
        MarginMode::Absolute => raw + value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> FxEngine {
        let mut e = FxEngine::empty("INR");
        let d = NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        e.rates.insert(("USD".into(), "INR".into()), RawRate { rate: 88.0, as_of: d });
        e.rates.insert(("USD".into(), "SGD".into()), RawRate { rate: 1.28, as_of: d });
        e.rates.insert(("USD".into(), "AUD".into()), RawRate { rate: 1.50, as_of: d });
        e
    }

    #[test]
    fn direct_rate_is_used_as_is() {
        let q = engine().quote("USD", "INR").unwrap();
        assert_eq!(q.effective_rate, 88.0);
        assert_eq!(q.source, "provider");
        assert!(!q.adjusted());
    }

    #[test]
    fn inverse_rate_is_derived() {
        let q = engine().quote("INR", "USD").unwrap();
        assert!((q.effective_rate - 1.0 / 88.0).abs() < 1e-12);
        assert_eq!(q.source, "inverse");
    }

    #[test]
    fn cross_rate_bridges_through_usd() {
        // SGD -> INR exists only as SGD -> USD -> INR.
        let q = engine().quote("SGD", "INR").unwrap();
        assert!((q.effective_rate - (1.0 / 1.28) * 88.0).abs() < 1e-9);
        assert_eq!(q.source, "cross:USD");
    }

    #[test]
    fn identity_never_needs_a_feed() {
        let q = FxEngine::empty("INR").quote("INR", "INR").unwrap();
        assert_eq!(q.effective_rate, 1.0);
    }

    #[test]
    fn missing_pair_returns_none_rather_than_one() {
        assert!(FxEngine::empty("INR").quote("USD", "INR").is_none());
    }

    #[test]
    fn margins_shift_the_rate_in_the_stated_direction() {
        assert!((apply_margin(88.0, "PERCENT", -0.5) - 87.56).abs() < 1e-9);
        assert!((apply_margin(88.0, "PERCENT", 1.0) - 88.88).abs() < 1e-9);
        assert_eq!(apply_margin(88.0, "ABSOLUTE", -0.25), 87.75);
        assert_eq!(apply_margin(88.0, "NONE", 99.0), 88.0);
    }

    #[test]
    fn manual_override_beats_the_feed_and_skips_the_margin() {
        let mut e = engine();
        e.settings.insert(
            ("USD".into(), "INR".into()),
            fx_setting::Model {
                id: 1,
                user_id: 1,
                base_code: "USD".into(),
                quote_code: "INR".into(),
                provider: "default".into(),
                margin_mode: "PERCENT".into(),
                margin_value: -10.0,
                manual_rate: Some(90.0),
                use_manual: true,
                note: None,
                updated_at: chrono::Utc::now().naive_utc(),
            },
        );
        let q = e.quote("USD", "INR").unwrap();
        assert_eq!(q.effective_rate, 90.0);
        assert_eq!(q.source, "manual");
    }

    /// The FX refresh stores every held currency against the base only, so
    /// USD -> SGD has to bridge through INR - past pivots that have no leg.
    fn base_only_engine() -> FxEngine {
        let mut e = FxEngine::empty("INR");
        let d = NaiveDate::from_ymd_opt(2026, 9, 18).unwrap();
        e.rates.insert(("USD".into(), "INR".into()), RawRate { rate: 88.0, as_of: d });
        e.rates.insert(("SGD".into(), "INR".into()), RawRate { rate: 68.75, as_of: d });
        e
    }

    #[test]
    fn a_cross_bridges_through_a_stored_currency_that_is_not_the_base() {
        // Rates against INR only, viewed in USD: SGD -> USD has no pivot
        // among USD, EUR, SGD or the base, so it has to go through INR.
        let mut e = base_only_engine();
        e.base = "USD".into();
        let q = e.quote("SGD", "USD").unwrap();
        assert!((q.effective_rate - 68.75 / 88.0).abs() < 1e-12);
        assert_eq!(q.source, "cross:INR");
        assert_eq!(e.to_base_or_flag(100.0, "SGD").fx_source, "cross:INR");
    }

    #[test]
    fn a_cross_skips_pivots_it_cannot_reach_and_bridges_through_the_base() {
        let q = base_only_engine().quote("USD", "SGD").unwrap();
        assert!((q.effective_rate - 88.0 / 68.75).abs() < 1e-12);
        assert_eq!(q.source, "cross:INR");
    }

    #[test]
    fn display_rates_cover_every_display_currency_and_gap_the_unknown() {
        let rates = base_only_engine().display_rates("AUD", 100.0);
        assert_eq!(rates.len(), DISPLAY_CURRENCIES.len());
        assert!(rates.iter().all(|r| r.rate.is_none()));
        let usd = base_only_engine().display_rates("USD", 100.0);
        assert_eq!(usd.iter().find(|r| r.currency == "USD").unwrap().rate, Some(1.0));
        assert_eq!(usd.iter().find(|r| r.currency == "INR").unwrap().rate, Some(88.0));
    }

    #[test]
    fn equivalents_convert_each_addend_from_its_own_currency() {
        let fx = base_only_engine();
        let mut e = Equivalents::default();
        e.add(100.0, "USD");
        e.add(1_000.0, "SGD");
        let out = e.rounded(&fx);
        assert_eq!(out["INR"], Some(8_800.0 + 68_750.0));
        assert_eq!(out["SGD"], Some(round_money(100.0 * 88.0 / 68.75 + 1_000.0)));

        e.add(5.0, "AUD");
        assert!(e.rounded(&fx).values().all(Option::is_none), "a part with no rate voids the total");
    }

    #[test]
    fn conversion_reports_the_rate_it_used() {
        let c = engine().to_base(1000.0, "USD").unwrap();
        assert_eq!(c.currency, "USD");
        assert_eq!(c.base_currency, "INR");
        assert_eq!(c.fx_rate, 88.0);
        assert_eq!(c.base_amount, 88_000.0);
    }

    /// Wise's SGD pricing on 24 Sep 2026: 0.33% up to about 30k, 0.229% on
    /// the slice above that.
    fn wise_sgd() -> FeeSchedule {
        FeeSchedule::new([(1_000.0, 3.29), (30_000.0, 98.67), (60_000.0, 169.58), (80_000.0, 215.48)]).unwrap()
    }

    fn wise_setting(base: &str, quote: &str) -> fx_setting::Model {
        fx_setting::Model {
            id: 1,
            user_id: 1,
            base_code: base.into(),
            quote_code: quote.into(),
            provider: "default".into(),
            margin_mode: "WISE".into(),
            margin_value: 0.0,
            manual_rate: None,
            use_manual: false,
            note: None,
            updated_at: chrono::Utc::now().naive_utc(),
        }
    }

    fn wise_engine() -> FxEngine {
        let mut e = base_only_engine();
        e.settings.insert(("SGD".into(), "INR".into()), wise_setting("SGD", "INR"));
        e.fees.insert(("SGD".into(), "INR".into()), wise_sgd());
        e
    }

    #[test]
    fn a_fee_schedule_interpolates_between_samples_and_carries_on_past_them() {
        let s = wise_sgd();
        assert!((s.fee(500.0) - 1.645).abs() < 1e-9, "below the first sample, pro rata from zero");
        assert!((s.fee(45_000.0) - (98.67 + 70.91 / 2.0)).abs() < 1e-9);
        // Past the last sample the last slice's rate continues.
        assert!((s.fee(100_000.0) - (215.48 + 45.9)).abs() < 1e-9);
        assert!((s.fraction(0.0) - 0.00329).abs() < 1e-12, "a tiny amount pays the first rate");
        assert!(s.fraction(80_000.0) < s.fraction(1_000.0), "larger amounts pay a smaller share");
        assert!(FeeSchedule::new([(0.0, 1.0), (-5.0, 1.0)]).is_none());
    }

    #[test]
    fn a_wise_pair_charges_the_fee_for_the_amount_converted() {
        let fx = wise_engine();
        let small = fx.convert_with_fee(1_000.0, "SGD", "INR").unwrap();
        assert!((small.base_amount - (1_000.0 - 3.29) * 68.75).abs() < 1e-6);
        assert!((small.fx_fee_pct.unwrap() - 0.329).abs() < 1e-9);
        assert!(small.fx_adjusted);

        let large = fx.convert_with_fee(80_000.0, "SGD", "INR").unwrap();
        assert!((large.base_amount - (80_000.0 - 215.48) * 68.75).abs() < 1e-6);
        assert!(large.fx_rate > small.fx_rate, "a larger amount gets the better rate");

        // The raw quote carries no fee - it has no amount to charge it on.
        assert_eq!(fx.quote("SGD", "INR").unwrap().effective_rate, 68.75);
        // The reverse direction has no setting of its own, so no fee.
        assert!(fx.to_base(1_000.0, "INR").is_some());
        assert_eq!(fx.convert_with_fee(68_750.0, "INR", "SGD").unwrap().fx_fee_pct, None);
    }

    #[test]
    fn holdings_are_valued_at_the_effective_rate_whatever_the_amount() {
        // The fee is a cost of converting, not of holding: every amount gets
        // the same rate, so totals add up whichever page reads them.
        let fx = wise_engine();
        for amount in [1_000.0, 80_000.0] {
            let c = fx.to_base(amount, "SGD").unwrap();
            assert_eq!(c.fx_rate, 68.75);
            assert_eq!(c.fx_fee_pct, None);
            assert!(!c.fx_adjusted);
        }
        let shown = fx.display_rates("SGD", 1_000.0);
        assert_eq!(shown.iter().find(|r| r.currency == "INR").unwrap().rate, Some(68.75));
    }

    #[test]
    fn a_wise_pair_without_fees_yet_or_with_a_manual_rate_converts_as_it_stands() {
        let mut fx = wise_engine();
        fx.fees.clear();
        let c = fx.to_base(1_000.0, "SGD").unwrap();
        assert_eq!(c.fx_rate, 68.75);
        assert_eq!(c.fx_fee_pct, None);

        let mut fx = wise_engine();
        let s = fx.settings.get_mut(&("SGD".to_string(), "INR".to_string())).unwrap();
        s.manual_rate = Some(70.0);
        s.use_manual = true;
        assert_eq!(fx.to_base(1_000.0, "SGD").unwrap().fx_rate, 70.0);
    }

    #[test]
    fn totals_use_the_effective_rate_without_the_fee() {
        let fx = wise_engine();
        let rates = TotalRates::new(&fx, [("SGD", 50_000.0), ("SGD", 30_000.0), ("INR", 10.0)]);
        assert_eq!(rates.rate("SGD"), 68.75);
        assert_eq!(rates.rate("INR"), 1.0);
        // No rate at all: flagged and counted 1:1, never dropped.
        let rates = TotalRates::new(&fx, [("AUD", 5.0)]);
        assert_eq!(rates.get("AUD").unwrap().fx_source, "unavailable");
    }

    #[test]
    fn equivalents_convert_each_currency_s_sum_as_one_amount() {
        let fx = wise_engine();
        let mut e = Equivalents::default();
        e.add(50_000.0, "SGD");
        e.add(30_000.0, "SGD");
        assert_eq!(e.rounded(&fx)["INR"], Some(round_money(80_000.0 * 68.75)));
    }

    #[test]
    fn display_totals_price_slices_at_the_page_rate_so_columns_add_up() {
        let fx = wise_engine();
        let page = DisplayTotals::new(&fx, [("SGD", 50_000.0), ("SGD", 30_000.0), ("INR", 1_000.0)]);
        let a = page.values([("SGD", 50_000.0)]);
        let b = page.values([("SGD", 30_000.0), ("INR", 1_000.0)]);
        let all = page.values([("SGD", 50_000.0), ("SGD", 30_000.0), ("INR", 1_000.0)]);
        let inr = |m: &BTreeMap<String, Option<f64>>| m["INR"].unwrap();
        assert!((inr(&a) + inr(&b) - inr(&all)).abs() < 0.02);
        assert_eq!(inr(&all), round_money(80_000.0 * 68.75 + 1_000.0));
        // A part with no rate leaves that column empty rather than 1:1.
        assert_eq!(page.values([("AUD", 5.0)])["INR"], None);
    }
}
