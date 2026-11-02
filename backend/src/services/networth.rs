//! The dashboard: one number for everything, and the breakdown behind it.

use chrono::NaiveDate;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

use crate::domain::fx::{Equivalents, FxEngine, TotalRates};
use crate::domain::money::{pct_change, round_money};
use crate::entities::fund_holding;
use crate::error::AppResult;
use crate::state::AppState;
use crate::types::CostBasisMethod;

use super::visibility::Excluded;
use super::{cash, deposits, portfolio, prefs};

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Segment {
    pub key: String,
    pub label: String,
    pub value_base: f64,
    pub invested_base: f64,
    pub gain_base: f64,
    pub gain_pct: Option<f64>,
    pub weight_pct: f64,
    /// The value in each display currency, every holding converted from its
    /// own currency. `None` where some part of it has no rate.
    pub values: BTreeMap<String, Option<f64>>,
    /// What the row is made of, by segment - a class's Indian funds apart
    /// from its global ETFs, say. Empty where the row is not broken down.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<Part>,
}

/// One segment's share of a class.
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Part {
    pub key: String,
    pub label: String,
    pub value_base: f64,
    /// Of net worth, like the row's own weight.
    pub weight_pct: f64,
    pub values: BTreeMap<String, Option<f64>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CurrencyExposure {
    pub currency: String,
    pub value_base: f64,
    pub weight_pct: f64,
    /// The exchange number used, shown next to every currency line.
    pub fx_rate: f64,
    pub fx_source: String,
    pub fx_as_of: Option<NaiveDate>,
    pub fx_adjusted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct NetWorth {
    pub base_currency: String,
    pub as_of: NaiveDate,
    pub total_base: f64,
    pub invested_base: f64,
    pub gain_base: f64,
    pub gain_pct: Option<f64>,
    /// The currencies `values` are given in, in display order.
    pub display_currencies: Vec<String>,
    /// Net worth in each display currency.
    pub total_values: BTreeMap<String, Option<f64>>,
    /// What went in, in each display currency - each holding's cost in its
    /// own currency, converted at today's rate, like `total_values`.
    pub invested_values: BTreeMap<String, Option<f64>>,
    /// What the money is in: funds, ETFs, stocks, deposits, cash.
    pub segments: Vec<Segment>,
    /// The same total on the equity-debt line, deposits and cash included.
    pub by_class: Vec<Segment>,
    pub currency_exposure: Vec<CurrencyExposure>,
    pub india_equity: portfolio::PortfolioSummary,
    pub india_mf: portfolio::PortfolioSummary,
    pub global: portfolio::PortfolioSummary,
    pub deposits: deposits::DepositSummary,
    pub cash: cash::CashSummary,
    /// Anything the user should know before trusting these numbers.
    pub warnings: Vec<String>,
    /// Pages the viewer has left out, by key (`global`, `cash`, ...): none
    /// of their value is in any figure above, and their summaries are empty.
    pub excluded: Vec<String>,
}

/// Segment keys and labels, in the order the dashboard lists them.
const SEGMENTS: &[(&str, &str)] = &[
    ("india_mf", "Indian Mutual Funds"),
    ("global_etf", "Global ETFs"),
    ("stocks", "Stocks"),
    ("deposits", "Deposits"),
    ("cash", "Cash"),
    ("other", "Other holdings"),
];

/// Classes on the equity-debt line, in order. Debt is split by what it
/// yields: funds, bonds and deposits above cash.
const CLASSES: &[(&str, &str)] = &[
    ("equity_fund", "Equity – ETF/Fund"),
    ("equity_stock", "Equity – Stocks"),
    ("debt_high", "Debt – High Yield"),
    ("debt_low", "Debt – Low Yield (Cash)"),
    ("gold", "Gold / Commodities"),
    ("other", "Other"),
];

/// Listed only when something is in it.
const OPTIONAL: &[&str] = &["other", "gold"];

/// One user's net worth, in their base currency.
pub async fn build(state: &AppState, owner: i32) -> AppResult<NetWorth> {
    let base = prefs::base_currency(state, owner).await;
    let fx = FxEngine::load(&state.db, &base, Some(owner)).await?;
    let method = prefs::cost_basis_method(state, owner).await;
    let today = chrono::Utc::now().date_naive();
    let excluded = Excluded::load(state, owner).await?;

    // An excluded page is built empty rather than skipped, so every figure
    // below - totals, splits, exposure, warnings - leaves it out alike.
    let india_equity = scope_view(state, owner, &fx, &excluded, portfolio::Scope::IndiaEquity, method).await?;
    let india_mf = scope_view(state, owner, &fx, &excluded, portfolio::Scope::IndiaMf, method).await?;
    let global = scope_view(state, owner, &fx, &excluded, portfolio::Scope::Global, method).await?;

    let deposit_views = if excluded.page("deposits") {
        Vec::new()
    } else {
        deposits::list(&state.db, owner, &fx, today).await?
    };
    let deposit_summary = deposits::summarise(&deposit_views, &base, today, &fx);

    let cash_views = if excluded.page("cash") { Vec::new() } else { cash::list(&state.db, owner, &fx, today).await? };
    let cash_summary = cash::summarise(&cash_views, &base, &fx);

    let views = [&india_equity, &india_mf, &global];
    let mixes = fund_mixes(state, &views).await?;

    // The dashboard's totals are priced per currency, as one conversion of
    // everything held in it - so a fee charged by amount is worked out on the
    // whole, not once per holding. Every figure below uses these rates.
    let rates = TotalRates::new(
        &fx,
        views
            .iter()
            .flat_map(|v| v.positions.iter())
            .map(|p| (p.currency.as_str(), p.market_value.unwrap_or(0.0)))
            .chain(deposit_views.iter().map(|d| (d.deposit.currency_code.as_str(), d.valuation.current_value)))
            .chain(cash_views.iter().map(|c| (c.currency.as_str(), c.balance))),
    );

    let mut segments = Split::new(SEGMENTS);
    // Each class is broken down by segment, so the equity in funds shows
    // how much of it is Indian and how much global.
    let mut classes = Split::new(CLASSES).with_parts(SEGMENTS);
    let mut total = Equivalents::default();
    let mut invested_total = Equivalents::default();
    // Native value per currency, for the exposure lines.
    let mut exposure_native: HashMap<String, f64> = HashMap::new();

    for p in views.iter().flat_map(|v| v.positions.iter()) {
        let native = p.market_value.unwrap_or(0.0);
        let segment = segment_of(p);
        segments.add(&rates, segment, segment, 1.0, native, p.invested, &p.currency);
        for (key, share) in class_shares(p, mixes.get(&p.instrument_id)) {
            classes.add(&rates, key, segment, share, native, p.invested, &p.currency);
        }
        total.add(native, &p.currency);
        invested_total.add(p.invested, &p.currency);
        *exposure_native.entry(p.currency.clone()).or_insert(0.0) += native;
    }
    for d in &deposit_views {
        let (native, invested) = (d.valuation.current_value, d.valuation.principal_invested);
        let ccy = d.deposit.currency_code.as_str();
        segments.add(&rates, "deposits", "deposits", 1.0, native, invested, ccy);
        classes.add(&rates, "debt_high", "deposits", 1.0, native, invested, ccy);
        total.add(native, ccy);
        invested_total.add(invested, ccy);
        *exposure_native.entry(ccy.to_string()).or_insert(0.0) += native;
    }
    for c in &cash_views {
        // Cash has no cost basis - it is worth what it is worth.
        segments.add(&rates, "cash", "cash", 1.0, c.balance, c.balance, &c.currency);
        classes.add(&rates, "debt_low", "cash", 1.0, c.balance, c.balance, &c.currency);
        total.add(c.balance, &c.currency);
        invested_total.add(c.balance, &c.currency);
        *exposure_native.entry(c.currency.clone()).or_insert(0.0) += c.balance;
    }

    let mut segments = segments.finish(&fx);
    // A row fed only by excluded pages would sit at a meaningless zero.
    segments.retain(|s| {
        let pages = segment_pages(&s.key);
        pages.is_empty() || !pages.iter().all(|p| excluded.page(p))
    });
    let by_class = classes.finish(&fx);
    let total_base: f64 = segments.iter().map(|s| s.value_base).sum();
    let invested: f64 = segments.iter().map(|s| s.invested_base).sum();

    // Currency exposure spans investments, deposits and cash - the whole
    // point of a multi-country portfolio is seeing this in one place.
    let mut currency_exposure: Vec<CurrencyExposure> = exposure_native
        .into_iter()
        .map(|(currency, native)| {
            let fallback = fx.to_base_or_flag(native, &currency);
            let c = rates.get(&currency).unwrap_or(&fallback);
            let value_base = round_money(native * c.fx_rate);
            CurrencyExposure {
                weight_pct: share_of(value_base, total_base),
                value_base,
                fx_rate: c.fx_rate,
                fx_source: c.fx_source.clone(),
                fx_as_of: c.fx_as_of,
                fx_adjusted: c.fx_adjusted,
                currency,
            }
        })
        .collect();
    currency_exposure.sort_by(|a, b| {
        b.value_base.partial_cmp(&a.value_base).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut warnings = Vec::new();
    let mut unconverted: Vec<String> = india_equity
        .summary
        .unconverted_currencies
        .iter()
        .chain(india_mf.summary.unconverted_currencies.iter())
        .chain(global.summary.unconverted_currencies.iter())
        .cloned()
        .collect();
    unconverted.sort();
    unconverted.dedup();
    if !unconverted.is_empty() {
        warnings.push(format!(
            "No exchange rate for {} - those holdings are counted at 1:1. Refresh rates or set a manual rate in Forex.",
            unconverted.join(", ")
        ));
    }
    let unpriced: usize = views
        .iter()
        .flat_map(|v| v.positions.iter())
        .filter(|p| p.last_price.is_none())
        .count();
    if unpriced > 0 {
        warnings.push(format!(
            "{unpriced} holding(s) have no price yet - their market value is excluded from totals."
        ));
    }
    if cash_summary.stale_accounts > 0 {
        warnings.push(format!(
            "{} bank balance(s) have not been updated in over 30 days.",
            cash_summary.stale_accounts
        ));
    }

    Ok(NetWorth {
        base_currency: base,
        as_of: today,
        total_base: round_money(total_base),
        invested_base: round_money(invested),
        gain_base: round_money(total_base - invested),
        gain_pct: pct_change(invested, total_base),
        display_currencies: fx.display_currencies(),
        total_values: total.rounded(&fx),
        invested_values: invested_total.rounded(&fx),
        segments,
        by_class,
        currency_exposure,
        india_equity: india_equity.summary,
        india_mf: india_mf.summary,
        global: global.summary,
        deposits: deposit_summary,
        cash: cash_summary,
        warnings,
        excluded: excluded.pages(),
    })
}

async fn scope_view(
    state: &AppState,
    owner: i32,
    fx: &FxEngine,
    excluded: &Excluded,
    scope: portfolio::Scope,
    method: CostBasisMethod,
) -> AppResult<portfolio::PortfolioView> {
    if excluded.scope(scope) {
        Ok(portfolio::PortfolioView::empty(fx, scope))
    } else {
        portfolio::build(&state.db, fx, Some(owner), scope, method, None, false).await
    }
}

/// The pages whose holdings can land in a segment. `other` has no page of
/// its own, so it is never dropped this way.
fn segment_pages(key: &str) -> &'static [&'static str] {
    match key {
        "india_mf" => &["india_mf"],
        "global_etf" => &["global"],
        "stocks" => &["india_equity", "global"],
        "deposits" => &["deposits"],
        "cash" => &["cash"],
        _ => &[],
    }
}

/// Sums per key, kept in a fixed order.
struct Split {
    rows: Vec<Row>,
    /// Where each row is broken down, the keys and labels of its parts, in
    /// the order they are listed.
    part_defs: Option<&'static [(&'static str, &'static str)]>,
}

struct Row {
    key: &'static str,
    label: &'static str,
    value_base: f64,
    invested_base: f64,
    values: Equivalents,
    /// Base value and per-currency value, by part key.
    parts: HashMap<&'static str, (f64, Equivalents)>,
}

impl Split {
    fn new(defs: &[(&'static str, &'static str)]) -> Self {
        Self {
            rows: defs
                .iter()
                .map(|(key, label)| Row {
                    key,
                    label,
                    value_base: 0.0,
                    invested_base: 0.0,
                    values: Equivalents::default(),
                    parts: HashMap::new(),
                })
                .collect(),
            part_defs: None,
        }
    }

    fn with_parts(mut self, defs: &'static [(&'static str, &'static str)]) -> Self {
        self.part_defs = Some(defs);
        self
    }

    /// `share` of a holding's value and cost, both in its own currency, goes
    /// to `key`, and within it to `part` where the split is broken down;
    /// the base figures use the page's rate for that currency.
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        rates: &TotalRates,
        key: &str,
        part: &'static str,
        share: f64,
        value: f64,
        invested: f64,
        currency: &str,
    ) {
        let row = self
            .rows
            .iter_mut()
            .find(|r| r.key == key)
            .expect("every key added is one of the split's own");
        let rate = rates.rate(currency);
        row.value_base += value * share * rate;
        row.invested_base += invested * share * rate;
        row.values.add(value * share, currency);
        if self.part_defs.is_some() {
            let (base, values) = row.parts.entry(part).or_default();
            *base += value * share * rate;
            values.add(value * share, currency);
        }
    }

    fn finish(self, fx: &FxEngine) -> Vec<Segment> {
        let total: f64 = self.rows.iter().map(|r| r.value_base).sum();
        let part_defs = self.part_defs.unwrap_or(&[]);
        self.rows
            .into_iter()
            .filter(|r| !OPTIONAL.contains(&r.key) || r.value_base.abs() > 0.005)
            .map(|mut r| {
                let parts = part_defs
                    .iter()
                    .filter_map(|(key, label)| {
                        let (base, values) = r.parts.remove(key)?;
                        let value = round_money(base);
                        (value.abs() > 0.005).then(|| Part {
                            key: key.to_string(),
                            label: label.to_string(),
                            value_base: value,
                            weight_pct: share_of(value, total),
                            values: values.rounded(fx),
                        })
                    })
                    .collect();
                let value = round_money(r.value_base);
                let invested = round_money(r.invested_base);
                Segment {
                    key: r.key.to_string(),
                    label: r.label.to_string(),
                    value_base: value,
                    invested_base: invested,
                    gain_base: round_money(value - invested),
                    gain_pct: pct_change(invested, value),
                    weight_pct: share_of(value, total),
                    values: r.values.rounded(fx),
                    parts,
                }
            })
            .collect()
    }
}

fn share_of(value: f64, total: f64) -> f64 {
    if total.abs() > f64::EPSILON {
        round_money(value / total * 100.0)
    } else {
        0.0
    }
}

fn is_india(p: &portfolio::PositionView) -> bool {
    p.country.as_deref().unwrap_or("IN").eq_ignore_ascii_case("IN")
}

/// Which segment a holding belongs to. Individual shares are one segment
/// wherever they are listed; funds split by where they are domiciled.
fn segment_of(p: &portfolio::PositionView) -> &'static str {
    match p.kind.as_str() {
        "MUTUAL_FUND" if is_india(p) => "india_mf",
        "ETF" | "MUTUAL_FUND" if !is_india(p) => "global_etf",
        "EQUITY" => "stocks",
        _ => "other",
    }
}

/// A fund's disclosed portfolio reduced to three shares that sum to one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mix {
    pub equity: f64,
    /// Bonds, money-market paper and the fund's own cash.
    pub debt: f64,
    /// Gold, units of other funds, anything else.
    pub rest: f64,
}

impl Mix {
    /// Negative lines (net payables) are dropped rather than netted, so no
    /// share goes below zero.
    pub fn of(rows: &[fund_holding::Model]) -> Option<Mix> {
        let (mut equity, mut debt, mut rest) = (0.0, 0.0, 0.0);
        for r in rows.iter().filter(|r| r.weight_pct > 0.0) {
            match r.nature.as_str() {
                "EQUITY" => equity += r.weight_pct,
                "DEBT" | "CASH" => debt += r.weight_pct,
                _ => rest += r.weight_pct,
            }
        }
        let total = equity + debt + rest;
        (total > 0.0).then(|| Mix { equity: equity / total, debt: debt / total, rest: rest / total })
    }
}

/// Disclosed mixes for the holdings whose class the portfolio decides:
/// ETFs and foreign funds, and Indian funds SEBI files as neither equity
/// nor debt.
async fn fund_mixes(
    state: &AppState,
    views: &[&portfolio::PortfolioView],
) -> AppResult<HashMap<i32, Mix>> {
    let ids: Vec<i32> = views
        .iter()
        .flat_map(|v| v.positions.iter())
        .filter(|p| needs_mix(p))
        .map(|p| p.instrument_id)
        .collect();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut by_fund: HashMap<i32, Vec<fund_holding::Model>> = HashMap::new();
    for r in fund_holding::Entity::find()
        .filter(fund_holding::Column::InstrumentId.is_in(ids))
        .all(&state.db)
        .await?
    {
        by_fund.entry(r.instrument_id).or_default().push(r);
    }
    Ok(by_fund.into_iter().filter_map(|(id, rows)| Mix::of(&rows).map(|m| (id, m))).collect())
}

fn needs_mix(p: &portfolio::PositionView) -> bool {
    match p.kind.as_str() {
        "ETF" => true,
        "MUTUAL_FUND" => !is_india(p) || !matches!(p.fund_class.as_deref(), Some("Equity" | "Debt")),
        _ => false,
    }
}

/// Where a holding sits on the equity-debt line, as shares of its value.
///
/// Indian funds go by the class SEBI gives them; a hybrid, or anything SEBI
/// files as neither, is split by its disclosed portfolio. ETFs and foreign
/// funds have no SEBI class, so they go whole to whichever side their
/// portfolio mostly holds - equity until one has been fetched, unless the
/// fund is plainly a metal or commodity one, which holds no shares at all.
/// What a metal or commodity fund holds that is neither equity nor debt -
/// the metal, or units of a fund holding it - is Gold / Commodities; for
/// any other fund it is Other.
fn class_shares(p: &portfolio::PositionView, mix: Option<&Mix>) -> Vec<(&'static str, f64)> {
    let rest = if is_commodity(p) { "gold" } else { "other" };
    match p.kind.as_str() {
        "EQUITY" => vec![("equity_stock", 1.0)],
        "BOND" => vec![("debt_high", 1.0)],
        "CASH" => vec![("debt_low", 1.0)],
        "REIT" => vec![("equity_fund", 1.0)],
        "COMMODITY" => vec![("gold", 1.0)],
        "MUTUAL_FUND" if is_india(p) => match (p.fund_class.as_deref(), mix) {
            (Some("Equity"), _) => vec![("equity_fund", 1.0)],
            (Some("Debt"), _) => vec![("debt_high", 1.0)],
            (_, Some(m)) => vec![("equity_fund", m.equity), ("debt_high", m.debt), (rest, m.rest)],
            (_, None) => vec![(rest, 1.0)],
        },
        "ETF" | "MUTUAL_FUND" => match mix {
            Some(m) if m.debt > m.equity && m.debt >= m.rest => vec![("debt_high", 1.0)],
            Some(m) if m.rest > m.equity && m.rest > m.debt => vec![(rest, 1.0)],
            None if is_commodity(p) => vec![("gold", 1.0)],
            _ => vec![("equity_fund", 1.0)],
        },
        _ => vec![("other", 1.0)],
    }
}

/// Gold, silver and other commodity funds, by name or category. A physical
/// gold ETF discloses no portfolio to go by, so this is all there is.
fn is_commodity(p: &portfolio::PositionView) -> bool {
    const WORDS: &[&str] = &["gold", "silver", "bullion", "commodit", "precious metal"];
    [Some(p.name.as_str()), p.fund_category.as_deref()]
        .into_iter()
        .flatten()
        .map(str::to_lowercase)
        .any(|t| WORDS.iter().any(|w| t.contains(w)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::portfolio::{FxView, PositionView};

    fn position(kind: &str, country: &str, fund_class: Option<&str>) -> PositionView {
        PositionView {
            account_id: 1,
            account_name: "A".into(),
            instrument_id: 1,
            symbol: "X".into(),
            name: "X".into(),
            kind: kind.into(),
            isin: None,
            exchange_code: None,
            country: Some(country.into()),
            sector: None,
            fund_class: fund_class.map(Into::into),
            fund_category: None,
            currency: "INR".into(),
            quantity: 1.0,
            average_cost: 1.0,
            invested: 1.0,
            last_price: Some(1.0),
            last_price_date: None,
            price_source: None,
            market_value: Some(1.0),
            unrealised_pnl: None,
            unrealised_pct: None,
            realised_pnl: 0.0,
            dividends: 0.0,
            fees_paid: 0.0,
            first_trade: None,
            xirr: None,
            fx: FxView {
                base_currency: "INR".into(),
                rate: 1.0,
                source: "identity".into(),
                as_of: None,
                adjusted: false,
                fee_pct: None,
            },
            invested_base: 1.0,
            market_value_base: Some(1.0),
            unrealised_pnl_base: None,
            realised_pnl_base: 0.0,
            dividends_base: 0.0,
            weight_pct: None,
        }
    }

    fn holding(nature: &str, weight: f64) -> fund_holding::Model {
        let d = NaiveDate::from_ymd_opt(2026, 8, 31).unwrap();
        fund_holding::Model {
            id: 0,
            instrument_id: 1,
            as_of: d,
            name: nature.into(),
            nature: nature.into(),
            instrument_type: None,
            sector: None,
            weight_pct: weight,
            market_value: None,
            security_ref: None,
            source: "groww".into(),
            created_at: d.and_hms_opt(0, 0, 0).unwrap(),
        }
    }

    #[test]
    fn segments_group_shares_together_and_funds_by_domicile() {
        assert_eq!(segment_of(&position("EQUITY", "IN", None)), "stocks");
        assert_eq!(segment_of(&position("EQUITY", "US", None)), "stocks");
        assert_eq!(segment_of(&position("MUTUAL_FUND", "IN", Some("Equity"))), "india_mf");
        assert_eq!(segment_of(&position("ETF", "US", None)), "global_etf");
        assert_eq!(segment_of(&position("ETF", "IN", None)), "other");
    }

    #[test]
    fn sebi_equity_and_debt_funds_go_whole() {
        let eq = position("MUTUAL_FUND", "IN", Some("Equity"));
        assert_eq!(class_shares(&eq, None), vec![("equity_fund", 1.0)]);
        let debt = position("MUTUAL_FUND", "IN", Some("Debt"));
        assert_eq!(class_shares(&debt, None), vec![("debt_high", 1.0)]);
        assert_eq!(class_shares(&position("EQUITY", "SG", None), None), vec![("equity_stock", 1.0)]);
    }

    #[test]
    fn a_hybrid_splits_by_its_disclosure_and_is_other_without_one() {
        let hybrid = position("MUTUAL_FUND", "IN", Some("Hybrid"));
        let mix = Mix::of(&[
            holding("EQUITY", 60.0),
            holding("DEBT", 30.0),
            holding("CASH", 12.0),
            holding("CASH", -2.0),
        ])
        .unwrap();
        let shares = class_shares(&hybrid, Some(&mix));
        assert_eq!(shares[0].0, "equity_fund");
        assert!((shares[0].1 - 60.0 / 102.0).abs() < 1e-12, "net payables are dropped, not netted");
        assert!((shares[1].1 - 42.0 / 102.0).abs() < 1e-12);
        assert_eq!(shares[2], ("other", 0.0));
        assert_eq!(class_shares(&hybrid, None), vec![("other", 1.0)]);
    }

    #[test]
    fn an_etf_goes_whole_to_the_side_its_portfolio_mostly_holds() {
        let etf = position("ETF", "US", None);
        assert_eq!(class_shares(&etf, None), vec![("equity_fund", 1.0)], "equity until fetched");
        let bonds = Mix::of(&[holding("DEBT", 97.0), holding("CASH", 3.0)]).unwrap();
        assert_eq!(class_shares(&etf, Some(&bonds)), vec![("debt_high", 1.0)]);
        let gold = Mix::of(&[holding("OTHER", 99.0), holding("CASH", 1.0)]).unwrap();
        assert_eq!(class_shares(&etf, Some(&gold)), vec![("other", 1.0)]);
    }

    #[test]
    fn a_gold_etf_is_gold_not_equity_or_other() {
        let mut gold = position("ETF", "SG", None);
        gold.name = "LION SG PHYSICAL GOLD ETF".into();
        assert_eq!(class_shares(&gold, None), vec![("gold", 1.0)]);
        // Metal it discloses holding is gold too.
        let metal = Mix::of(&[holding("OTHER", 99.0), holding("CASH", 1.0)]).unwrap();
        assert_eq!(class_shares(&gold, Some(&metal)), vec![("gold", 1.0)]);
        // A disclosed portfolio still wins over the name.
        let stocks = Mix::of(&[holding("EQUITY", 100.0)]).unwrap();
        assert_eq!(class_shares(&gold, Some(&stocks)), vec![("equity_fund", 1.0)]);
        let mut silver = position("MUTUAL_FUND", "LU", None);
        silver.fund_category = Some("Precious Metals".into());
        assert_eq!(class_shares(&silver, None), vec![("gold", 1.0)]);
        assert_eq!(class_shares(&position("COMMODITY", "GB", None), None), vec![("gold", 1.0)]);
    }

    #[test]
    fn an_indian_gold_fund_of_funds_is_gold() {
        let mut fof = position("MUTUAL_FUND", "IN", Some("Other"));
        fof.name = "SBI Gold Fund".into();
        assert_eq!(class_shares(&fof, None), vec![("gold", 1.0)]);
        let units = Mix::of(&[holding("OTHER", 98.0), holding("CASH", 2.0)]).unwrap();
        let shares = class_shares(&fof, Some(&units));
        assert_eq!(shares[2].0, "gold");
        assert!((shares[2].1 - 0.98).abs() < 1e-12);
    }

    #[test]
    fn the_gold_row_is_dropped_only_when_empty() {
        let fx = FxEngine::empty("INR");
        let rates = TotalRates::new(&fx, [("INR", 100.0)]);
        let mut s = Split::new(CLASSES);
        s.add(&rates, "equity_fund", "global_etf", 1.0, 100.0, 100.0, "INR");
        let keys: Vec<_> = s.finish(&fx).into_iter().map(|r| r.key).collect();
        assert!(!keys.iter().any(|k| k == "gold" || k == "other"), "{keys:?}");
        let mut s = Split::new(CLASSES);
        s.add(&rates, "gold", "global_etf", 1.0, 100.0, 100.0, "INR");
        assert!(s.finish(&fx).iter().any(|r| r.key == "gold"));
    }

    #[test]
    fn a_class_is_broken_down_by_segment() {
        let fx = FxEngine::empty("INR");
        let rates = TotalRates::new(&fx, [("INR", 400.0)]);
        let mut s = Split::new(CLASSES).with_parts(SEGMENTS);
        s.add(&rates, "equity_fund", "global_etf", 1.0, 100.0, 90.0, "INR");
        s.add(&rates, "equity_fund", "india_mf", 1.0, 200.0, 150.0, "INR");
        s.add(&rates, "debt_high", "deposits", 1.0, 100.0, 100.0, "INR");
        let out = s.finish(&fx);
        let parts: Vec<_> = out[0].parts.iter().map(|p| (p.key.as_str(), p.value_base, p.weight_pct)).collect();
        assert_eq!(parts, vec![("india_mf", 200.0, 50.0), ("global_etf", 100.0, 25.0)], "in segment order");
        assert_eq!(out[2].parts.len(), 1);
        assert!(out[1].parts.is_empty(), "nothing held, nothing listed");
    }

    #[test]
    fn the_optional_row_is_dropped_only_when_empty() {
        let fx = FxEngine::empty("INR");
        let rates = TotalRates::new(&fx, [("INR", 100.0)]);
        let mut s = Split::new(SEGMENTS);
        s.add(&rates, "cash", "cash", 1.0, 100.0, 100.0, "INR");
        assert!(s.part_defs.is_none());
        let out = s.finish(&fx);
        assert_eq!(out.len(), SEGMENTS.len() - 1);
        assert_eq!(out[4].key, "cash");
        assert_eq!(out[4].weight_pct, 100.0);
        assert_eq!(out[4].values["INR"], Some(100.0));
        assert_eq!(out[4].values["USD"], None, "no rate, no figure");
        assert_eq!(out[0].values["USD"], Some(0.0), "nothing held needs no rate");
        assert!(out[4].parts.is_empty(), "segments are not broken down");
    }
}
