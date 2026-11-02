//! Assembles positions from transactions, prices and FX.
//!
//! Positions are computed per (account, instrument) pair, because cost basis
//! is an account-level fact: FIFO lots in a Zerodha demat account are not
//! interchangeable with lots of the same stock held at IBKR.

use std::collections::HashMap;
use std::str::FromStr;

use chrono::NaiveDate;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, Statement,
};
use serde::{Deserialize, Serialize};

use crate::domain::fx::{DisplayRate, FxEngine, TotalRates};
use crate::domain::holdings::{unrealised, CashFlow, LotBook};
use crate::domain::money::{pct_change, round_money, round_to, ConvertedMoney};
use crate::domain::scheme;
use crate::domain::xirr::xirr;
use crate::entities::{account, exchange, instrument, price_point, txn};
use crate::error::AppResult;
use crate::types::{AssetKind, CostBasisMethod};

/// Which slice of the portfolio a request is asking about. These map onto the
/// UI's tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    All,
    /// Indian listed equity, ETFs and REITs.
    IndiaEquity,
    /// Indian mutual-fund schemes.
    IndiaMf,
    /// Everything listed outside India.
    Global,
}

impl Scope {
    pub fn matches(&self, inst: &instrument::Model) -> bool {
        let kind = AssetKind::from_str(&inst.kind).unwrap_or(AssetKind::Equity);
        let is_india = inst.country.as_deref().unwrap_or("IN").eq_ignore_ascii_case("IN");
        match self {
            Scope::All => true,
            Scope::IndiaMf => is_india && kind == AssetKind::MutualFund,
            Scope::IndiaEquity => {
                is_india && matches!(kind, AssetKind::Equity | AssetKind::Etf | AssetKind::Reit)
            }
            Scope::Global => !is_india,
        }
    }
}

impl FromStr for Scope {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "all" | "" => Ok(Scope::All),
            "india_equity" | "india-equity" => Ok(Scope::IndiaEquity),
            "india_mf" | "india-mf" => Ok(Scope::IndiaMf),
            "global" => Ok(Scope::Global),
            other => Err(format!("unknown scope '{other}'")),
        }
    }
}

/// The exchange-rate facts shown beside every non-base figure.
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FxView {
    pub base_currency: String,
    /// Units of base per 1 unit of the position's currency.
    pub rate: f64,
    pub source: String,
    pub as_of: Option<NaiveDate>,
    /// A margin or manual override moved this off the raw feed.
    pub adjusted: bool,
    /// The transfer fee taken off this figure, as a percentage of it, where
    /// the pair charges one by amount.
    pub fee_pct: Option<f64>,
}

impl FxView {
    pub fn of(c: &ConvertedMoney) -> Self {
        Self {
            base_currency: c.base_currency.clone(),
            rate: c.fx_rate,
            source: c.fx_source.clone(),
            as_of: c.fx_as_of,
            adjusted: c.fx_adjusted,
            fee_pct: c.fx_fee_pct,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PositionView {
    pub account_id: i32,
    pub account_name: String,
    pub instrument_id: i32,
    pub symbol: String,
    pub name: String,
    pub kind: String,
    pub isin: Option<String>,
    pub exchange_code: Option<String>,
    pub country: Option<String>,
    /// For funds, the AMFI category heading (`Equity Scheme - Large Cap
    /// Fund`); for equities, the sector.
    pub sector: Option<String>,
    /// Funds only: `Equity`, `Debt`, `Hybrid` or `Other`.
    pub fund_class: Option<String>,
    /// Funds only: the SEBI sub-category, e.g. `Large Cap`, `Money Market`.
    pub fund_category: Option<String>,

    /// Currency the instrument trades in - always shown next to the numbers.
    pub currency: String,
    pub quantity: f64,
    pub average_cost: f64,
    pub invested: f64,
    pub last_price: Option<f64>,
    pub last_price_date: Option<NaiveDate>,
    pub price_source: Option<String>,
    pub market_value: Option<f64>,
    pub unrealised_pnl: Option<f64>,
    pub unrealised_pct: Option<f64>,
    pub realised_pnl: f64,
    pub dividends: f64,
    pub fees_paid: f64,
    pub first_trade: Option<NaiveDate>,
    /// Money-weighted annualised return, including any realised legs.
    pub xirr: Option<f64>,

    /// The exchange number, carried alongside rather than folded away.
    pub fx: FxView,
    pub invested_base: f64,
    pub market_value_base: Option<f64>,
    pub unrealised_pnl_base: Option<f64>,
    pub realised_pnl_base: f64,
    pub dividends_base: f64,
    /// Share of the scope's total market value, 0-100.
    pub weight_pct: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Bucket {
    pub key: String,
    pub label: String,
    pub value_base: f64,
    pub invested_base: f64,
    pub weight_pct: f64,
    /// Populated for currency buckets so the Forex figure is visible here too.
    pub fx_rate: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PortfolioSummary {
    pub base_currency: String,
    pub scope: Scope,
    pub positions: usize,
    pub invested_base: f64,
    pub market_value_base: f64,
    pub unrealised_pnl_base: f64,
    pub unrealised_pct: Option<f64>,
    pub realised_pnl_base: f64,
    pub dividends_base: f64,
    pub xirr: Option<f64>,
    pub by_currency: Vec<Bucket>,
    pub by_asset_class: Vec<Bucket>,
    pub by_country: Vec<Bucket>,
    /// The same totals in each currency the holdings trade in, unconverted,
    /// largest first - with what one unit of each is worth in the display
    /// currencies.
    pub by_source_currency: Vec<CurrencyTotals>,
    /// Currencies with holdings but no usable FX rate - shown as a warning
    /// rather than silently counted at 1:1.
    pub unconverted_currencies: Vec<String>,
}

/// The holdings that trade in one currency, summed in that currency.
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CurrencyTotals {
    pub currency: String,
    pub positions: usize,
    pub market_value: f64,
    pub invested: f64,
    pub unrealised_pnl: f64,
    pub unrealised_pct: Option<f64>,
    pub realised_pnl: f64,
    pub dividends: f64,
    /// For ordering, and for the share of the scope.
    pub market_value_base: f64,
    pub rates: Vec<DisplayRate>,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PortfolioView {
    pub summary: PortfolioSummary,
    pub positions: Vec<PositionView>,
}

impl PortfolioView {
    /// Nothing held: for a scope the viewer has left out of the totals.
    pub fn empty(fx: &FxEngine, scope: Scope) -> Self {
        Self { summary: summarise(fx, scope, &mut [], &[]), positions: Vec::new() }
    }
}

/// Reference data loaded once per request.
struct Context {
    accounts: HashMap<i32, account::Model>,
    instruments: HashMap<i32, instrument::Model>,
    exchanges: HashMap<i32, exchange::Model>,
    /// instrument_id -> most recent price point.
    prices: HashMap<i32, price_point::Model>,
}

async fn load_context(db: &DatabaseConnection, owner: Option<i32>) -> AppResult<Context> {
    let mut accounts = account::Entity::find();
    if let Some(user) = owner {
        accounts = accounts.filter(account::Column::UserId.eq(user));
    }
    let accounts = accounts
        .all(db)
        .await?
        .into_iter()
        .map(|a| (a.id, a))
        .collect();
    let instruments = instrument::Entity::find()
        .all(db)
        .await?
        .into_iter()
        .map(|i| (i.id, i))
        .collect();
    let exchanges = exchange::Entity::find()
        .all(db)
        .await?
        .into_iter()
        .map(|e| (e.id, e))
        .collect();

    // Only the newest row per instrument is wanted. With daily history
    // backfilled the table runs to tens of thousands of rows, so let the
    // database pick the latest rather than sorting the lot and keeping the
    // last write - that sort is exactly the kind of spill that needs a temp
    // file SQLite may not have.
    let latest = price_point::Entity::find()
        .from_raw_sql(Statement::from_string(
            db.get_database_backend(),
            "SELECT p.* FROM price_point p \
             INNER JOIN ( \
                 SELECT instrument_id, MAX(as_of) AS as_of \
                 FROM price_point GROUP BY instrument_id \
             ) newest ON newest.instrument_id = p.instrument_id AND newest.as_of = p.as_of",
        ))
        .all(db)
        .await?;
    let mut prices = HashMap::new();
    for p in latest {
        prices.insert(p.instrument_id, p);
    }

    Ok(Context { accounts, instruments, exchanges, prices })
}

/// Build the full view for a scope, of one user's accounts - or, with no
/// `owner`, of every account, which only the market-data refresh asks for
/// (to learn which funds anyone holds).
pub async fn build(
    db: &DatabaseConnection,
    fx: &FxEngine,
    owner: Option<i32>,
    scope: Scope,
    method: CostBasisMethod,
    account_filter: Option<i32>,
    include_closed: bool,
) -> AppResult<PortfolioView> {
    let ctx = load_context(db, owner).await?;

    let mut query = txn::Entity::find()
        .order_by_asc(txn::Column::TradeDate)
        .order_by_asc(txn::Column::Id);
    if let Some(acc) = account_filter {
        query = query.filter(txn::Column::AccountId.eq(acc));
    }
    if owner.is_some() {
        let ids: Vec<i32> = ctx.accounts.keys().copied().collect();
        query = query.filter(txn::Column::AccountId.is_in(ids));
    }
    let all_txns = query.all(db).await?;

    // Group by (account, instrument) - one lot book per pair.
    let mut grouped: HashMap<(i32, i32), Vec<txn::Model>> = HashMap::new();
    for t in all_txns {
        grouped.entry((t.account_id, t.instrument_id)).or_default().push(t);
    }

    let today = chrono::Utc::now().date_naive();
    let mut positions = Vec::new();
    let mut portfolio_flows: Vec<CashFlow> = Vec::new();

    for ((account_id, instrument_id), txns) in grouped {
        let (Some(inst), Some(acct)) = (ctx.instruments.get(&instrument_id), ctx.accounts.get(&account_id))
        else {
            // Orphaned transaction - the FK makes this near-impossible, but
            // skipping beats panicking on a hand-edited database.
            continue;
        };
        if !scope.matches(inst) {
            continue;
        }

        let mut book = LotBook::new(method);
        book.apply_all(&txns);
        let (basis, mut flows) = book.finish();

        if !include_closed && basis.quantity.abs() < 1e-9 {
            continue;
        }

        let price_row = ctx.prices.get(&instrument_id);
        let last_price = price_row.map(|p| p.close);
        let (market_value, pnl, pnl_pct) = unrealised(&basis, last_price);

        // The open position is a notional inflow today - that is what makes
        // XIRR meaningful for a position still being held.
        if let Some(mv) = market_value {
            if mv.abs() > 1e-9 {
                flows.push(CashFlow { date: today, amount: mv });
            }
        }
        // One rate for the whole row, priced on what the holding is worth
        // (or cost, before it has a price): a fee charged by amount is
        // worked out as if the holding were converted in one go.
        let converted =
            fx.to_base_or_flag(market_value.unwrap_or(basis.invested), &inst.currency_code);
        let rate = converted.fx_rate;

        let position_xirr = xirr(&flows);
        // Re-base each flow into the portfolio currency before mixing
        // currencies in one XIRR, otherwise the maths is meaningless.
        if converted.fx_source != "unavailable" {
            portfolio_flows
                .extend(flows.iter().map(|f| CashFlow { date: f.date, amount: f.amount * rate }));
        }

        let exchange_code = inst
            .exchange_id
            .and_then(|id| ctx.exchanges.get(&id))
            .map(|e| e.code.clone());

        let fund = (AssetKind::from_str(&inst.kind) == Ok(AssetKind::MutualFund))
            .then(|| scheme::classify(inst.sector.as_deref(), &inst.name));

        positions.push(PositionView {
            account_id,
            account_name: acct.name.clone(),
            instrument_id,
            symbol: inst.symbol.clone(),
            name: inst.name.clone(),
            kind: inst.kind.clone(),
            isin: inst.isin.clone(),
            exchange_code,
            country: inst.country.clone(),
            sector: inst.sector.clone(),
            fund_class: fund.as_ref().map(|f| f.class.as_str().to_string()),
            fund_category: fund.as_ref().and_then(|f| f.category.clone()),
            currency: inst.currency_code.clone(),
            quantity: basis.quantity,
            // Full precision, so quantity x average cost gives back what was invested.
            average_cost: round_to(basis.average_cost, 6),
            invested: basis.invested,
            last_price,
            last_price_date: price_row.map(|p| p.as_of),
            price_source: price_row.map(|p| p.source.clone()),
            market_value,
            unrealised_pnl: pnl,
            unrealised_pct: pnl_pct,
            realised_pnl: basis.realised_pnl,
            dividends: basis.dividends,
            fees_paid: round_money(basis.fees_paid),
            first_trade: basis.first_trade,
            xirr: position_xirr,
            fx: FxView::of(&converted),
            invested_base: round_money(basis.invested * rate),
            market_value_base: market_value.map(|v| round_money(v * rate)),
            unrealised_pnl_base: pnl.map(|v| round_money(v * rate)),
            realised_pnl_base: round_money(basis.realised_pnl * rate),
            dividends_base: round_money(basis.dividends * rate),
            weight_pct: None,
        });
    }

    // Biggest holdings first - the order people actually want to read.
    positions.sort_by(|a, b| {
        b.market_value_base
            .unwrap_or(0.0)
            .partial_cmp(&a.market_value_base.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let summary = summarise(fx, scope, &mut positions, &portfolio_flows);
    Ok(PortfolioView { summary, positions })
}

fn summarise(
    fx: &FxEngine,
    scope: Scope,
    positions: &mut [PositionView],
    flows: &[CashFlow],
) -> PortfolioSummary {
    // Totals are priced per currency, as one conversion of everything held
    // in it - not as the sum of each row's own conversion - so a fee charged
    // by amount is worked out on the whole. Every aggregate below uses these.
    let rates = TotalRates::new(
        fx,
        positions.iter().map(|p| (p.currency.as_str(), p.market_value.unwrap_or(0.0))),
    );
    let in_base = |p: &PositionView, amount: f64| amount * rates.rate(&p.currency);
    let total_value: f64 = positions.iter().map(|p| in_base(p, p.market_value.unwrap_or(0.0))).sum();
    let invested: f64 = positions.iter().map(|p| in_base(p, p.invested)).sum();
    let realised: f64 = positions.iter().map(|p| in_base(p, p.realised_pnl)).sum();
    let dividends: f64 = positions.iter().map(|p| in_base(p, p.dividends)).sum();

    for p in positions.iter_mut() {
        p.weight_pct = match (p.market_value_base, total_value) {
            (Some(v), t) if t.abs() > f64::EPSILON => Some(round_money(v / t * 100.0)),
            _ => None,
        };
    }

    let unconverted: Vec<String> = {
        let mut v: Vec<String> = positions
            .iter()
            .filter(|p| p.fx.source == "unavailable")
            .map(|p| p.currency.clone())
            .collect();
        v.sort();
        v.dedup();
        v
    };

    PortfolioSummary {
        base_currency: fx.base().to_string(),
        scope,
        positions: positions.len(),
        invested_base: round_money(invested),
        market_value_base: round_money(total_value),
        unrealised_pnl_base: round_money(total_value - invested),
        unrealised_pct: pct_change(invested, total_value),
        realised_pnl_base: round_money(realised),
        dividends_base: round_money(dividends),
        xirr: xirr(flows),
        by_currency: bucket_by(positions, &rates, total_value, |p| {
            (p.currency.clone(), p.currency.clone(), Some(rates.rate(&p.currency)))
        }),
        by_asset_class: bucket_by(positions, &rates, total_value, |p| {
            (p.kind.clone(), humanise(&p.kind), None)
        }),
        by_country: bucket_by(positions, &rates, total_value, |p| {
            let c = p.country.clone().unwrap_or_else(|| "??".to_string());
            (c.clone(), c, None)
        }),
        by_source_currency: currency_totals(fx, &rates, positions),
        unconverted_currencies: unconverted,
    }
}

/// Totals per trading currency. The same arithmetic as the base-currency
/// summary - unrealised is value less everything invested - so a single
/// currency's figures agree with the scope's.
fn currency_totals(fx: &FxEngine, rates: &TotalRates, positions: &[PositionView]) -> Vec<CurrencyTotals> {
    let mut map: HashMap<String, CurrencyTotals> = HashMap::new();
    for p in positions {
        let t = map.entry(p.currency.clone()).or_insert_with(|| CurrencyTotals {
            currency: p.currency.clone(),
            positions: 0,
            market_value: 0.0,
            invested: 0.0,
            unrealised_pnl: 0.0,
            unrealised_pct: None,
            realised_pnl: 0.0,
            dividends: 0.0,
            market_value_base: 0.0,
            rates: Vec::new(),
        });
        t.positions += 1;
        t.market_value += p.market_value.unwrap_or(0.0);
        t.invested += p.invested;
        t.realised_pnl += p.realised_pnl;
        t.dividends += p.dividends;
        t.market_value_base += p.market_value.unwrap_or(0.0) * rates.rate(&p.currency);
    }
    let mut out: Vec<CurrencyTotals> = map.into_values().collect();
    for t in out.iter_mut() {
        // At the rate converting the whole of this currency's holdings would get.
        t.rates = fx.display_rates(&t.currency, t.market_value);
        t.unrealised_pnl = round_money(t.market_value - t.invested);
        t.unrealised_pct = pct_change(t.invested, t.market_value);
        t.market_value = round_money(t.market_value);
        t.invested = round_money(t.invested);
        t.realised_pnl = round_money(t.realised_pnl);
        t.dividends = round_money(t.dividends);
        t.market_value_base = round_money(t.market_value_base);
    }
    out.sort_by(|a, b| {
        b.market_value_base
            .partial_cmp(&a.market_value_base)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.currency.cmp(&b.currency))
    });
    out
}

fn bucket_by<F>(positions: &[PositionView], rates: &TotalRates, total: f64, key: F) -> Vec<Bucket>
where
    F: Fn(&PositionView) -> (String, String, Option<f64>),
{
    let mut map: HashMap<String, Bucket> = HashMap::new();
    for p in positions {
        let (k, label, fx_rate) = key(p);
        let entry = map.entry(k.clone()).or_insert(Bucket {
            key: k,
            label,
            value_base: 0.0,
            invested_base: 0.0,
            weight_pct: 0.0,
            fx_rate,
        });
        let rate = rates.rate(&p.currency);
        entry.value_base += p.market_value.unwrap_or(0.0) * rate;
        entry.invested_base += p.invested * rate;
    }
    let mut out: Vec<Bucket> = map.into_values().collect();
    for b in out.iter_mut() {
        b.value_base = round_money(b.value_base);
        b.invested_base = round_money(b.invested_base);
        b.weight_pct = if total.abs() > f64::EPSILON {
            round_money(b.value_base / total * 100.0)
        } else {
            0.0
        };
    }
    out.sort_by(|a, b| b.value_base.partial_cmp(&a.value_base).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// `MUTUAL_FUND` -> `Mutual Fund`.
fn humanise(code: &str) -> String {
    code.split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inst(kind: &str, country: Option<&str>) -> instrument::Model {
        instrument::Model {
            id: 1,
            kind: kind.into(),
            symbol: "X".into(),
            name: "X".into(),
            isin: None,
            exchange_id: None,
            currency_code: "INR".into(),
            country: country.map(|c| c.into()),
            quote_symbol: None,
            amfi_scheme_code: None,
            sector: None,
            history_from: None,
            holdings_ref: None,
            is_active: true,
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
        }
    }

    #[test]
    fn scopes_split_india_from_the_rest() {
        assert!(Scope::IndiaEquity.matches(&inst("EQUITY", Some("IN"))));
        assert!(!Scope::IndiaEquity.matches(&inst("MUTUAL_FUND", Some("IN"))));
        assert!(Scope::IndiaMf.matches(&inst("MUTUAL_FUND", Some("IN"))));
        assert!(Scope::Global.matches(&inst("ETF", Some("US"))));
        assert!(!Scope::Global.matches(&inst("ETF", Some("IN"))));
        assert!(Scope::All.matches(&inst("BOND", Some("SG"))));
    }

    #[test]
    fn instruments_with_no_country_are_treated_as_indian() {
        assert!(Scope::IndiaEquity.matches(&inst("EQUITY", None)));
        assert!(!Scope::Global.matches(&inst("EQUITY", None)));
    }

    #[test]
    fn scope_parsing_accepts_both_separators() {
        assert_eq!(Scope::from_str("india-mf").unwrap(), Scope::IndiaMf);
        assert_eq!(Scope::from_str("INDIA_EQUITY").unwrap(), Scope::IndiaEquity);
        assert_eq!(Scope::from_str("").unwrap(), Scope::All);
        assert!(Scope::from_str("mars").is_err());
    }

    #[test]
    fn humanises_enum_codes() {
        assert_eq!(humanise("MUTUAL_FUND"), "Mutual Fund");
        assert_eq!(humanise("ETF"), "Etf");
    }
}
