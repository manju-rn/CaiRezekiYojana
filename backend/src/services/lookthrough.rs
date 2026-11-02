//! Look-through: the securities inside the mutual funds, combined.
//!
//! A fund is a wrapper. Holding four equity funds is not four positions -
//! it is a slice of every stock each of them holds, and the same bank turns
//! up in all four. This module opens the wrappers: each fund's disclosed
//! portfolio (`fund_holding`, fetched by a `HoldingsProvider`) is weighted
//! by what the user's units in that fund are worth today, and the slices
//! are summed per security.
//!
//! The result is an estimate and says so. Disclosures are monthly, so the
//! weights are up to a month or two old and are applied to today's fund
//! value; a fund with no disclosure stored is listed as uncovered rather
//! than quietly left out of the total.
//!
//! The same view serves the global tab, where the wrappers are ETFs and
//! the shares held directly sit beside them: a stock bought outright and
//! the same stock inside an ETF are one line, and its sector one bar.

use std::cmp::Ordering;
use std::collections::HashMap;

use chrono::{NaiveDate, NaiveDateTime};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, TransactionTrait,
};
use serde::{Deserialize, Serialize};

use crate::domain::fx::FxEngine;
use crate::domain::money::{round_money, round_to};
use crate::entities::{exchange, fund_holding, instrument};
use crate::error::AppResult;
use crate::providers::quotes_yahoo::provider_symbol;
use crate::providers::{self, holdings_yahoo, FundPortfolio, HoldingsProvider, REMAINDER_TYPE};
use crate::state::AppState;

use super::prefs;
use crate::types::CostBasisMethod;
use super::portfolio::{self, PositionView, Scope};
use super::refresh::{corrected_kind, set_kind, RefreshReport};

/// Disclosures are monthly. Asking the feed again within this many days of
/// the last fetch would return the same month, so the refresh skips a fund
/// fetched more recently unless forced.
pub const REFETCH_AFTER_DAYS: i64 = 3;

/// A disclosure older than this is flagged: a newer month is out and the
/// feed has not caught up, or the fund has stopped disclosing.
const STALE_AFTER_DAYS: i64 = 75;

/// Column widths in `fund_holding`. SQLite ignores them; Postgres does not,
/// and a debt line's full description can run past the name column.
const NAME_MAX: usize = 240;
const NATURE_MAX: usize = 16;
const TYPE_MAX: usize = 80;
const REF_MAX: usize = 120;

/// Rows per bulk insert - 11 columns each, comfortably under any parameter cap.
const INSERT_BATCH: usize = 150;

/// One fund's contribution to a security.
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FundSlice {
    pub instrument_id: i32,
    pub fund_name: String,
    /// What the user's units in the fund are worth, in base currency.
    pub fund_value_base: f64,
    /// The security's share of the fund's assets, 0-100.
    pub weight_pct: f64,
    /// The user's slice: fund value times weight.
    pub value_base: f64,
    /// The disclosure the weight came from.
    pub as_of: NaiveDate,
    /// Not a fund at all: the security itself, bought outright.
    pub direct: bool,
}

/// One security, summed across every fund that holds it.
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Exposure {
    /// Stable identity across funds: the feed's security id, else the
    /// normalised name.
    pub key: String,
    pub name: String,
    /// `EQUITY`, `DEBT`, `CASH`, `MF` or `OTHER`.
    pub nature: String,
    pub instrument_type: Option<String>,
    pub sector: Option<String>,
    pub value_base: f64,
    /// Share of the covered fund value, 0-100.
    pub weight_pct: f64,
    pub fund_count: usize,
    /// Largest contribution first.
    pub funds: Vec<FundSlice>,
    /// Stands for the part of a portfolio the feed does not itemise, not
    /// for one security.
    pub remainder: bool,
}

/// Whether, and how recently, each held fund's portfolio is known.
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FundCoverage {
    pub instrument_id: i32,
    pub name: String,
    pub market_value_base: f64,
    /// Rows stored; zero means the fund is not covered.
    pub holdings: usize,
    pub as_of: Option<NaiveDate>,
    pub source: Option<String>,
    pub fetched_at: Option<NaiveDateTime>,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Slice {
    pub key: String,
    pub label: String,
    pub value_base: f64,
    pub weight_pct: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct LookThrough {
    pub scope: Scope,
    /// What every figure is in: the base currency unless another was asked for.
    pub base_currency: String,
    /// The valuation date: today's fund values, not the disclosure date.
    pub as_of: NaiveDate,
    /// Every priced fund position, covered or not.
    pub fund_value_base: f64,
    /// The part of it with a disclosed portfolio.
    pub covered_value_base: f64,
    /// Securities held outright alongside the funds. The exposures sum to
    /// this plus the covered value.
    pub direct_value_base: f64,
    /// Largest fund first.
    pub funds: Vec<FundCoverage>,
    /// Largest first.
    pub exposures: Vec<Exposure>,
    pub by_nature: Vec<Slice>,
    /// Equity only, by sector.
    pub by_sector: Vec<Slice>,
    pub warnings: Vec<String>,
}

/// A fund the user holds, valued: what the aggregation weights by.
#[derive(Clone, Debug, PartialEq)]
pub struct FundValue {
    pub instrument_id: i32,
    pub name: String,
    pub market_value_base: f64,
}

/// A security held outright, valued: on the global tab it joins the
/// securities inside the ETFs.
#[derive(Clone, Debug, PartialEq)]
pub struct DirectHolding {
    pub instrument_id: i32,
    pub name: String,
    pub nature: String,
    pub sector: Option<String>,
    /// The quote symbol, which is what the ETF feed names its holdings by.
    pub security_ref: Option<String>,
    pub market_value_base: f64,
}

/// Which positions are wrappers to open. The funds tab holds nothing else;
/// on the global tab the rest are held directly.
fn is_fund(kind: &str) -> bool {
    matches!(kind, "ETF" | "MUTUAL_FUND")
}

/// `currency` reports in something other than the base - the global tab
/// asks for the currency its holdings trade in.
pub async fn build(state: &AppState, owner: i32, scope: Scope, currency: Option<String>) -> AppResult<LookThrough> {
    let base = match currency {
        Some(c) => c,
        None => prefs::base_currency(state, owner).await,
    };
    let fx = FxEngine::load(&state.db, &base, Some(owner)).await?;
    let method = prefs::cost_basis_method(state, owner).await;
    let view = portfolio::build(&state.db, &fx, Some(owner), scope, method, None, false).await?;

    let (fund_positions, direct_positions): (Vec<PositionView>, Vec<PositionView>) =
        view.positions.into_iter().partition(|p| is_fund(&p.kind));
    let direct = if direct_positions.is_empty() {
        Vec::new()
    } else {
        direct_holdings(&state.db, &direct_positions).await?
    };

    let funds = fund_values(&fund_positions);
    let ids: Vec<i32> = funds.iter().map(|f| f.instrument_id).collect();
    let rows = if ids.is_empty() {
        Vec::new()
    } else {
        fund_holding::Entity::find()
            .filter(fund_holding::Column::InstrumentId.is_in(ids))
            .all(&state.db)
            .await?
    };
    let mut by_fund: HashMap<i32, Vec<fund_holding::Model>> = HashMap::new();
    for r in rows {
        by_fund.entry(r.instrument_id).or_default().push(r);
    }

    Ok(assemble(scope, &base, chrono::Utc::now().date_naive(), funds, by_fund, direct))
}

/// One line per security held outright, summed across accounts, named by
/// its quote symbol so it meets the same stock inside an ETF.
async fn direct_holdings(db: &DatabaseConnection, positions: &[PositionView]) -> AppResult<Vec<DirectHolding>> {
    let ids: Vec<i32> = positions.iter().map(|p| p.instrument_id).collect();
    let instruments: HashMap<i32, instrument::Model> = instrument::Entity::find()
        .filter(instrument::Column::Id.is_in(ids))
        .all(db)
        .await?
        .into_iter()
        .map(|i| (i.id, i))
        .collect();
    let exchanges: HashMap<i32, exchange::Model> =
        exchange::Entity::find().all(db).await?.into_iter().map(|e| (e.id, e)).collect();

    let mut by_id: HashMap<i32, DirectHolding> = HashMap::new();
    for p in positions {
        let Some(mv) = p.market_value_base else { continue };
        let inst = instruments.get(&p.instrument_id);
        let h = by_id.entry(p.instrument_id).or_insert_with(|| DirectHolding {
            instrument_id: p.instrument_id,
            name: p.name.clone(),
            nature: match p.kind.as_str() {
                "BOND" => "DEBT",
                "CASH" => "CASH",
                "COMMODITY" => "OTHER",
                _ => "EQUITY",
            }
            .to_string(),
            sector: p.sector.clone(),
            security_ref: inst.map(|i| provider_symbol(i, &exchanges)),
            market_value_base: 0.0,
        });
        h.market_value_base += mv;
    }
    let mut out: Vec<DirectHolding> = by_id.into_values().collect();
    out.sort_by(|a, b| desc(a.market_value_base, b.market_value_base).then_with(|| a.name.cmp(&b.name)));
    Ok(out)
}

/// One value per fund. Positions are per folio, and the same scheme in two
/// folios is still one wrapper around one portfolio. Unpriced positions
/// contribute nothing - there is no value to apportion.
pub fn fund_values(positions: &[PositionView]) -> Vec<FundValue> {
    let mut by_id: HashMap<i32, FundValue> = HashMap::new();
    for p in positions {
        let Some(mv) = p.market_value_base else { continue };
        let f = by_id.entry(p.instrument_id).or_insert_with(|| FundValue {
            instrument_id: p.instrument_id,
            name: p.name.clone(),
            market_value_base: 0.0,
        });
        f.market_value_base += mv;
    }
    let mut out: Vec<FundValue> = by_id.into_values().collect();
    out.sort_by(|a, b| desc(a.market_value_base, b.market_value_base).then_with(|| a.name.cmp(&b.name)));
    out
}

/// The pure part: each fund's value spread over its holdings by weight,
/// then summed per security, with the securities held outright added in.
pub fn assemble(
    scope: Scope,
    base: &str,
    today: NaiveDate,
    funds: Vec<FundValue>,
    mut holdings: HashMap<i32, Vec<fund_holding::Model>>,
    direct: Vec<DirectHolding>,
) -> LookThrough {
    let mut exposures: HashMap<String, Exposure> = HashMap::new();
    let mut coverage = Vec::with_capacity(funds.len());
    let mut fund_total = 0.0;
    let mut covered = 0.0;
    let mut uncovered: Vec<String> = Vec::new();
    let mut stale: Vec<String> = Vec::new();

    for f in &funds {
        fund_total += f.market_value_base;
        let rows = holdings.remove(&f.instrument_id).unwrap_or_default();
        let as_of = rows.iter().map(|r| r.as_of).max();
        coverage.push(FundCoverage {
            instrument_id: f.instrument_id,
            name: f.name.clone(),
            market_value_base: round_money(f.market_value_base),
            holdings: rows.len(),
            as_of,
            source: rows.first().map(|r| r.source.clone()),
            fetched_at: rows.iter().map(|r| r.created_at).max(),
        });
        if rows.is_empty() {
            uncovered.push(f.name.clone());
            continue;
        }
        covered += f.market_value_base;
        if let Some(d) = as_of {
            if (today - d).num_days() > STALE_AFTER_DAYS {
                stale.push(format!("{} ({d})", f.name));
            }
        }

        for r in rows {
            let value = f.market_value_base * r.weight_pct / 100.0;
            let key = exposure_key(&r);
            let e = exposures.entry(key.clone()).or_insert_with(|| Exposure {
                key,
                name: r.name.clone(),
                nature: r.nature.clone(),
                instrument_type: r.instrument_type.clone(),
                sector: r.sector.clone(),
                value_base: 0.0,
                weight_pct: 0.0,
                fund_count: 0,
                funds: Vec::new(),
                remainder: r.instrument_type.as_deref() == Some(REMAINDER_TYPE),
            });
            e.value_base += value;
            // The first fund to name a sector or type for the security wins;
            // a later one filling a blank is still welcome.
            if e.sector.is_none() {
                e.sector = r.sector.clone();
            }
            if e.instrument_type.is_none() {
                e.instrument_type = r.instrument_type.clone();
            }
            e.funds.push(FundSlice {
                instrument_id: f.instrument_id,
                fund_name: f.name.clone(),
                fund_value_base: round_money(f.market_value_base),
                weight_pct: r.weight_pct,
                value_base: round_money(value),
                as_of: r.as_of,
                direct: false,
            });
        }
    }

    // After the funds, so a stock both held and inside an ETF takes the
    // feed's sector - the name the rest of that sector's stocks go by.
    let mut direct_total = 0.0;
    for d in direct {
        direct_total += d.market_value_base;
        let key = match d.security_ref.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(id) => format!("ref:{id}"),
            None => format!("name:{}", normalise_name(&d.name)),
        };
        let e = exposures.entry(key.clone()).or_insert_with(|| Exposure {
            key,
            name: d.name.clone(),
            nature: d.nature.clone(),
            instrument_type: None,
            sector: None,
            value_base: 0.0,
            weight_pct: 0.0,
            fund_count: 0,
            funds: Vec::new(),
            remainder: false,
        });
        e.value_base += d.market_value_base;
        if e.sector.is_none() {
            e.sector = d.sector.clone();
        }
        e.funds.push(FundSlice {
            instrument_id: d.instrument_id,
            fund_name: "Held directly".to_string(),
            fund_value_base: round_money(d.market_value_base),
            weight_pct: 100.0,
            value_base: round_money(d.market_value_base),
            as_of: today,
            direct: true,
        });
    }

    let mut exposures: Vec<Exposure> = exposures.into_values().collect();
    for e in exposures.iter_mut() {
        e.fund_count = e.funds.len();
        e.funds.sort_by(|a, b| desc(a.value_base, b.value_base).then_with(|| a.fund_name.cmp(&b.fund_name)));
        e.value_base = round_money(e.value_base);
        e.weight_pct = share(e.value_base, covered + direct_total);
    }
    exposures.sort_by(|a, b| desc(a.value_base, b.value_base).then_with(|| a.name.cmp(&b.name)));

    let by_nature = slices(exposures.iter(), |e| (e.nature.clone(), nature_label(&e.nature)));
    let by_sector = slices(exposures.iter().filter(|e| e.nature == "EQUITY"), |e| {
        let s = e.sector.clone().unwrap_or_else(|| "Unclassified".to_string());
        (s.clone(), s)
    });

    let mut warnings = Vec::new();
    if !uncovered.is_empty() {
        warnings.push(format!(
            "{} fund{} without a stored portfolio, left out of the combined figures: {}. \
             A refresh fetches the missing disclosures.",
            uncovered.len(),
            if uncovered.len() == 1 { "" } else { "s" },
            list(&uncovered)
        ));
    }
    if !stale.is_empty() {
        warnings.push(format!(
            "Portfolio disclosures more than {STALE_AFTER_DAYS} days old for {}.",
            list(&stale)
        ));
    }

    LookThrough {
        scope,
        base_currency: base.to_string(),
        as_of: today,
        fund_value_base: round_money(fund_total),
        covered_value_base: round_money(covered),
        direct_value_base: round_money(direct_total),
        funds: coverage,
        exposures,
        by_nature,
        by_sector,
        warnings,
    }
}

/// `EQUITY` -> `Stocks`, for legends and warnings.
pub fn nature_label(nature: &str) -> String {
    match nature {
        "EQUITY" => "Stocks",
        "DEBT" => "Bonds & money market",
        "CASH" => "Cash & receivables",
        "MF" => "Other funds",
        _ => "Other",
    }
    .to_string()
}

/// What makes two rows the same security: the feed's own id where it gives
/// one (equities), otherwise the name, normalised so `HDFC Bank Ltd.` and
/// `HDFC BANK LIMITED` agree.
fn exposure_key(r: &fund_holding::Model) -> String {
    match r.security_ref.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => format!("ref:{id}"),
        None => format!("name:{}", normalise_name(&r.name)),
    }
}

/// Lowercase alphanumeric words, single-spaced, without a trailing
/// `Ltd`/`Limited`.
pub fn normalise_name(name: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            cur.push(c.to_ascii_lowercase());
        } else if !cur.is_empty() {
            words.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    while words.last().is_some_and(|w| w == "ltd" || w == "limited") {
        words.pop();
    }
    words.join(" ")
}

fn slices<'a, I, F>(items: I, key: F) -> Vec<Slice>
where
    I: Iterator<Item = &'a Exposure>,
    F: Fn(&Exposure) -> (String, String),
{
    let mut map: HashMap<String, Slice> = HashMap::new();
    for e in items {
        let (k, label) = key(e);
        let s = map.entry(k.clone()).or_insert(Slice { key: k, label, value_base: 0.0, weight_pct: 0.0 });
        s.value_base += e.value_base;
    }
    let total: f64 = map.values().map(|s| s.value_base).sum();
    let mut out: Vec<Slice> = map.into_values().collect();
    for s in out.iter_mut() {
        s.value_base = round_money(s.value_base);
        s.weight_pct = share(s.value_base, total);
    }
    out.sort_by(|a, b| desc(a.value_base, b.value_base).then_with(|| a.label.cmp(&b.label)));
    out
}

fn share(value: f64, total: f64) -> f64 {
    if total.abs() > f64::EPSILON {
        round_to(value / total * 100.0, 4)
    } else {
        0.0
    }
}

/// Largest first. Callers break ties by name, so equal values - two
/// securities at the same weight in one fund - keep a stable order.
fn desc(a: f64, b: f64) -> Ordering {
    b.partial_cmp(&a).unwrap_or(Ordering::Equal)
}

/// The first few names, then a count.
fn list(names: &[String]) -> String {
    const SHOWN: usize = 4;
    if names.len() <= SHOWN {
        names.join(", ")
    } else {
        format!("{} and {} more", names[..SHOWN].join(", "), names.len() - SHOWN)
    }
}

// ---------------------------------------------------------------------------
// Refresh
// ---------------------------------------------------------------------------

/// Fetch and store the disclosed portfolio of every fund the user holds in
/// `scope` (or of `only` those), skipping funds fetched within
/// [`REFETCH_AFTER_DAYS`] unless `force`. Indian funds come from the
/// disclosure feed in settings, ETFs listed abroad from Yahoo. Gated by the
/// `fund_holdings_enabled` setting. Like every refresh, a feed failure is
/// a warning in the report, never an error out of it.
pub async fn refresh_fund_holdings(
    state: &AppState,
    scope: Scope,
    only: Option<Vec<i32>>,
    force: bool,
) -> AppResult<RefreshReport> {
    let provider: Box<dyn HoldingsProvider> = match scope {
        Scope::Global => providers::etf_holdings_provider(),
        _ => providers::holdings_provider(&state.setting("holdings_provider", "groww").await),
    };
    let mut report = RefreshReport {
        source: format!("{}_portfolios", provider.id()),
        ..Default::default()
    };
    if state.setting("fund_holdings_enabled", "true").await != "true" {
        return Ok(report);
    }

    // Only funds actually held, by anyone: a redeemed fund's portfolio is
    // nobody's exposure, and every fund costs the feed a request or three.
    // Which lots are open does not depend on the method, nor on the rates.
    let fx = FxEngine::load(&state.db, "INR", None).await?;
    let view = portfolio::build(&state.db, &fx, None, scope, CostBasisMethod::Fifo, None, false).await?;
    let refiled = if scope == Scope::Global {
        classify_shares(state, &view.positions, &mut report).await?
    } else {
        Vec::new()
    };
    let mut ids: Vec<i32> = view
        .positions
        .iter()
        .filter(|p| is_fund(&p.kind))
        .map(|p| p.instrument_id)
        .chain(refiled)
        .collect();
    if let Some(only) = &only {
        ids.retain(|id| only.contains(id));
    }
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return Ok(report);
    }

    let funds = instrument::Entity::find()
        .filter(instrument::Column::Id.is_in(ids))
        .filter(instrument::Column::IsActive.eq(true))
        .order_by_asc(instrument::Column::Name)
        .all(&state.db)
        .await?;
    let now = chrono::Utc::now().naive_utc();

    for fund in funds {
        if !force {
            let last = fund_holding::Entity::find()
                .filter(fund_holding::Column::InstrumentId.eq(fund.id))
                .order_by_desc(fund_holding::Column::CreatedAt)
                .one(&state.db)
                .await?;
            if last.is_some_and(|r| (now - r.created_at).num_days() < REFETCH_AFTER_DAYS) {
                continue;
            }
        }
        report.requested += 1;

        match provider.portfolio(state, &fund).await {
            Ok(Some(p)) => {
                store_portfolio(&state.db, &fund, &p, provider.id()).await?;
                report.updated += 1;
                report.as_of = Some(report.as_of.map_or(p.as_of, |d| d.max(p.as_of)));
            }
            Ok(None) => {
                report.missing += 1;
                let fix = if scope == Scope::Global {
                    "set its quote symbol, or the portfolio reference on the instrument"
                } else {
                    "set its AMFI scheme code, or the portfolio reference on the instrument"
                };
                report.warnings.push(format!(
                    "no portfolio found for '{}' on {} - {fix}, to fix",
                    fund.name,
                    provider.id()
                ));
            }
            Err(e) => {
                report.missing += 1;
                report.warnings.push(format!("{}: {e}", fund.name));
            }
        }
    }

    Ok(report)
}

/// Give each share held outright the sector Yahoo files it under, when the
/// import gave it none - so it lands in the same sector bar as the ETFs'
/// stocks. A sector already on the instrument is the user's and is kept.
///
/// A "share" Yahoo calls an ETF was imported without its asset kind; it is
/// refiled as an ETF instead, and its id returned so its portfolio is
/// fetched in the same refresh.
async fn classify_shares(
    state: &AppState,
    positions: &[PositionView],
    report: &mut RefreshReport,
) -> AppResult<Vec<i32>> {
    let mut refiled = Vec::new();
    let mut ids: Vec<i32> = positions
        .iter()
        .filter(|p| matches!(p.kind.as_str(), "EQUITY" | "REIT") && p.sector.is_none())
        .map(|p| p.instrument_id)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return Ok(refiled);
    }
    let exchanges: HashMap<i32, exchange::Model> =
        exchange::Entity::find().all(&state.db).await?.into_iter().map(|e| (e.id, e)).collect();
    for inst in instrument::Entity::find().filter(instrument::Column::Id.is_in(ids)).all(&state.db).await? {
        let symbol = provider_symbol(&inst, &exchanges);
        let found = match holdings_yahoo::profile(state, &symbol).await {
            Ok(p) => p.unwrap_or_default(),
            Err(e) => {
                report.warnings.push(format!("{symbol}: sector lookup failed: {e}"));
                break;
            }
        };
        if let Some(kind) = corrected_kind(&inst.kind, found.quote_type.as_deref()) {
            set_kind(&state.db, &inst, kind).await?;
            report.warnings.push(format!("{symbol}: Yahoo lists it as an {kind}, so it is now filed as one"));
            refiled.push(inst.id);
            continue;
        }
        match found.sector {
            Some(sector) => {
                let mut m: instrument::ActiveModel = inst.into();
                m.sector = Set(Some(clip(&sector, TYPE_MAX)));
                m.updated_at = Set(chrono::Utc::now().naive_utc());
                m.update(&state.db).await?;
            }
            None => report.warnings.push(format!("{symbol}: Yahoo gives no sector - set one on the instrument")),
        }
    }
    Ok(refiled)
}

fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Replace the fund's stored portfolio with this disclosure, and remember
/// the feed's reference for the fund. One transaction, so a failure
/// half-way leaves the previous disclosure in place rather than nothing.
async fn store_portfolio(
    db: &DatabaseConnection,
    fund: &instrument::Model,
    portfolio: &FundPortfolio,
    source: &str,
) -> AppResult<()> {
    let txn = db.begin().await?;

    fund_holding::Entity::delete_many()
        .filter(fund_holding::Column::InstrumentId.eq(fund.id))
        .filter(fund_holding::Column::Source.eq(source))
        .exec(&txn)
        .await?;

    let now = chrono::Utc::now().naive_utc();
    let clip = |s: &str, max: usize| -> String { s.chars().take(max).collect() };
    let rows: Vec<fund_holding::ActiveModel> = portfolio
        .holdings
        .iter()
        .map(|h| fund_holding::ActiveModel {
            instrument_id: Set(fund.id),
            as_of: Set(portfolio.as_of),
            name: Set(clip(&h.name, NAME_MAX)),
            nature: Set(clip(&h.nature, NATURE_MAX)),
            instrument_type: Set(h.instrument_type.as_deref().map(|s| clip(s, TYPE_MAX))),
            sector: Set(h.sector.as_deref().map(|s| clip(s, TYPE_MAX))),
            weight_pct: Set(h.weight_pct),
            market_value: Set(h.market_value),
            security_ref: Set(h.security_ref.as_deref().map(|s| clip(s, REF_MAX))),
            source: Set(source.to_string()),
            created_at: Set(now),
            ..Default::default()
        })
        .collect();
    let mut pending = rows.into_iter().peekable();
    while pending.peek().is_some() {
        let batch: Vec<fund_holding::ActiveModel> = pending.by_ref().take(INSERT_BATCH).collect();
        fund_holding::Entity::insert_many(batch).exec(&txn).await?;
    }

    if fund.holdings_ref.as_deref() != Some(portfolio.fund_ref.as_str()) {
        let mut m: instrument::ActiveModel = fund.clone().into();
        m.holdings_ref = Set(Some(portfolio.fund_ref.clone()));
        m.updated_at = Set(now);
        m.update(&txn).await?;
    }

    txn.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn row(
        fund: i32,
        name: &str,
        nature: &str,
        sector: Option<&str>,
        weight: f64,
        security_ref: Option<&str>,
        as_of: NaiveDate,
    ) -> fund_holding::Model {
        fund_holding::Model {
            id: 0,
            instrument_id: fund,
            as_of,
            name: name.into(),
            nature: nature.into(),
            instrument_type: Some(if nature == "EQUITY" { "Equity" } else { "Repo" }.into()),
            sector: sector.map(Into::into),
            weight_pct: weight,
            market_value: None,
            security_ref: security_ref.map(Into::into),
            source: "groww".into(),
            created_at: as_of.and_hms_opt(18, 0, 0).unwrap(),
        }
    }

    fn fund(id: i32, name: &str, value: f64) -> FundValue {
        FundValue { instrument_id: id, name: name.into(), market_value_base: value }
    }

    #[test]
    fn a_stock_held_by_two_funds_is_one_line_with_the_sum() {
        let aug = d(2026, 8, 31);
        let mut holdings = HashMap::new();
        holdings.insert(
            1,
            vec![
                row(1, "HDFC Bank Ltd", "EQUITY", Some("Financial"), 10.0, Some("hdfc-bank-ltd"), aug),
                row(1, "Repo", "CASH", None, 2.0, None, aug),
            ],
        );
        holdings.insert(
            2,
            vec![
                row(2, "HDFC Bank Ltd", "EQUITY", Some("Financial"), 5.0, Some("hdfc-bank-ltd"), aug),
                row(2, "Infosys Ltd", "EQUITY", Some("Technology"), 4.0, Some("infosys-ltd"), aug),
            ],
        );

        let lt = assemble(
            Scope::IndiaMf,
            "INR",
            d(2026, 9, 22),
            vec![fund(1, "Large Cap", 1_000_000.0), fund(2, "Flexi Cap", 500_000.0)],
            holdings,
            vec![],
        );

        assert_eq!(lt.fund_value_base, 1_500_000.0);
        assert_eq!(lt.covered_value_base, 1_500_000.0);
        assert!(lt.warnings.is_empty());

        let hdfc = &lt.exposures[0];
        assert_eq!(hdfc.name, "HDFC Bank Ltd");
        assert_eq!(hdfc.value_base, 100_000.0 + 25_000.0);
        assert_eq!(hdfc.fund_count, 2);
        assert_eq!(hdfc.funds[0].fund_name, "Large Cap", "largest slice first");
        assert_eq!(hdfc.funds[0].value_base, 100_000.0);
        assert_eq!(hdfc.funds[1].weight_pct, 5.0);
        assert_eq!(hdfc.weight_pct, round_to(125_000.0 / 1_500_000.0 * 100.0, 4));

        let names: Vec<&str> = lt.exposures.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["HDFC Bank Ltd", "Infosys Ltd", "Repo"]);

        assert_eq!(lt.by_nature[0].label, "Stocks");
        assert_eq!(lt.by_nature[0].value_base, 145_000.0);
        assert_eq!(lt.by_nature[1].label, "Cash & receivables");
        // Sector shares are of the equity slice, not of everything.
        assert_eq!(lt.by_sector[0].key, "Financial");
        assert_eq!(lt.by_sector[0].weight_pct, round_to(125_000.0 / 145_000.0 * 100.0, 4));
    }

    #[test]
    fn an_uncovered_fund_is_counted_in_the_total_but_flagged_and_left_out_of_coverage() {
        let mut holdings = HashMap::new();
        holdings.insert(1, vec![row(1, "Repo", "CASH", None, 100.0, None, d(2026, 8, 31))]);
        let lt = assemble(
            Scope::IndiaMf,
            "INR",
            d(2026, 9, 22),
            vec![fund(1, "Liquid", 100.0), fund(2, "No Disclosure Yet", 900.0)],
            holdings,
            vec![],
        );
        assert_eq!(lt.fund_value_base, 1000.0);
        assert_eq!(lt.covered_value_base, 100.0);
        assert_eq!(lt.funds.len(), 2);
        assert_eq!(lt.funds[0].name, "Liquid", "the order given is kept");
        assert_eq!(lt.funds[1].holdings, 0);
        assert_eq!(lt.warnings.len(), 1);
        assert!(lt.warnings[0].contains("No Disclosure Yet"));
    }

    #[test]
    fn an_old_disclosure_is_flagged() {
        let mut holdings = HashMap::new();
        holdings.insert(1, vec![row(1, "Repo", "CASH", None, 100.0, None, d(2026, 3, 31))]);
        let lt = assemble(Scope::IndiaMf, "INR", d(2026, 9, 22), vec![fund(1, "Liquid", 100.0)], holdings, vec![]);
        assert_eq!(lt.warnings.len(), 1);
        assert!(lt.warnings[0].contains("Liquid (2026-03-31)"));
    }

    #[test]
    fn without_a_feed_id_the_name_is_the_identity() {
        assert_eq!(normalise_name("HDFC Bank Ltd."), "hdfc bank");
        assert_eq!(normalise_name("HDFC  BANK LIMITED"), "hdfc bank");
        assert_eq!(normalise_name("INDUSIND BANK LTD. CD 29JAN27"), "indusind bank ltd cd 29jan27");
        let a = row(1, "HDFC Bank Ltd.", "EQUITY", None, 1.0, None, d(2026, 8, 31));
        let b = row(2, "HDFC BANK LIMITED", "EQUITY", None, 1.0, None, d(2026, 8, 31));
        assert_eq!(exposure_key(&a), exposure_key(&b));
        let c = row(2, "HDFC Bank Ltd", "EQUITY", None, 1.0, Some("hdfc-bank-ltd"), d(2026, 8, 31));
        assert_eq!(exposure_key(&c), "ref:hdfc-bank-ltd");
    }

    #[test]
    fn negative_weights_reduce_the_exposure() {
        let aug = d(2026, 8, 31);
        let mut holdings = HashMap::new();
        holdings.insert(
            1,
            vec![
                row(1, "Repo", "CASH", None, 108.0, None, aug),
                row(1, "Net Payables", "CASH", None, -8.0, None, aug),
            ],
        );
        let lt = assemble(Scope::IndiaMf, "INR", d(2026, 9, 22), vec![fund(1, "Liquid", 1000.0)], holdings, vec![]);
        let total: f64 = lt.exposures.iter().map(|e| e.value_base).sum();
        assert_eq!(total, 1000.0);
        assert_eq!(lt.exposures[1].value_base, -80.0);
    }

    #[test]
    fn a_stock_held_outright_and_inside_an_etf_is_one_line_in_the_etf_sector() {
        let today = d(2026, 9, 22);
        let mut etf_rows = vec![
            row(1, "DBS Group Holdings Ltd", "EQUITY", Some("Financial Services"), 25.0, Some("D05.SI"), today),
            row(1, "Other Financial Services holdings", "EQUITY", Some("Financial Services"), 75.0, None, today),
        ];
        etf_rows[1].instrument_type = Some(REMAINDER_TYPE.into());
        let mut holdings = HashMap::new();
        holdings.insert(1, etf_rows);
        let direct = vec![
            DirectHolding {
                instrument_id: 7,
                name: "DBS Group Holdings".into(),
                nature: "EQUITY".into(),
                sector: Some("Banks".into()),
                security_ref: Some("D05.SI".into()),
                market_value_base: 3_000.0,
            },
            DirectHolding {
                instrument_id: 8,
                name: "Microsoft Corp".into(),
                nature: "EQUITY".into(),
                sector: None,
                security_ref: Some("MSFT".into()),
                market_value_base: 1_000.0,
            },
        ];

        let lt = assemble(Scope::Global, "SGD", today, vec![fund(1, "STI ETF", 4_000.0)], holdings, direct);
        assert_eq!(lt.scope, Scope::Global);
        assert_eq!(lt.covered_value_base, 4_000.0);
        assert_eq!(lt.direct_value_base, 4_000.0);

        let dbs = lt.exposures.iter().find(|e| e.key == "ref:D05.SI").unwrap();
        assert_eq!(dbs.value_base, 1_000.0 + 3_000.0);
        assert_eq!(dbs.sector.as_deref(), Some("Financial Services"), "the feed's sector wins");
        assert_eq!(dbs.fund_count, 2);
        assert!(dbs.funds.iter().any(|f| f.direct && f.fund_name == "Held directly"));
        assert_eq!(dbs.weight_pct, 50.0, "a share of funds and direct holdings together");

        let rest = lt.exposures.iter().find(|e| e.remainder).unwrap();
        assert_eq!(rest.value_base, 3_000.0);
        assert!(!dbs.remainder);

        let msft = lt.exposures.iter().find(|e| e.key == "ref:MSFT").unwrap();
        assert_eq!(msft.sector, None);
        assert_eq!(lt.by_sector.iter().map(|s| s.value_base).sum::<f64>(), 8_000.0);
        assert!(lt.funds.iter().all(|f| f.instrument_id == 1), "direct holdings are not funds");
    }

    #[test]
    fn long_lists_are_abbreviated() {
        let names: Vec<String> = (1..=6).map(|i| format!("Fund {i}")).collect();
        assert_eq!(list(&names), "Fund 1, Fund 2, Fund 3, Fund 4 and 2 more");
        assert_eq!(list(&names[..2]), "Fund 1, Fund 2");
    }
}
