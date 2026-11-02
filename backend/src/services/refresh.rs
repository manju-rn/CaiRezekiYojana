//! Pulling fresh prices, NAVs and FX rates in from the providers.
//!
//! Every refresh is best-effort and idempotent. A provider outage produces a
//! report full of warnings and leaves the last good values in the database -
//! the portfolio never goes blank because Yahoo had a bad afternoon.

use std::collections::{HashMap, HashSet};

use chrono::{Datelike, NaiveDate};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder,
};
use serde::Serialize;

use crate::domain::fx::{FxEngine, DISPLAY_CURRENCIES};
use crate::domain::scheme;
use crate::auth::Role;
use crate::entities::{account, app_user, exchange, fx_fee, fx_rate, fx_setting, instrument, price_point, txn};
use crate::services::prefs;
use crate::error::AppResult;
use crate::providers::{self, fx_fees_wise, nav_history_mfapi, quotes_yahoo, quotes_yahoo::compose_symbol};
use crate::state::AppState;
use crate::types::MarginMode;

#[derive(Debug, Default, Serialize, utoipa::ToSchema)]
pub struct RefreshReport {
    pub source: String,
    /// How many things we set out to refresh.
    pub requested: usize,
    /// Rows written (inserted or updated).
    pub updated: usize,
    /// Asked for but not returned by the provider.
    pub missing: usize,
    pub warnings: Vec<String>,
    pub as_of: Option<NaiveDate>,
    /// FX pairs whose transfer fees were re-sampled. Only the FX refresh sets it.
    pub fees_updated: usize,
}

// ---------------------------------------------------------------------------
// Equity / ETF prices
// ---------------------------------------------------------------------------

pub async fn refresh_quotes(state: &AppState, only: Option<Vec<i32>>) -> AppResult<RefreshReport> {
    let provider_id = state.setting("quote_provider", "yahoo").await;
    let provider = providers::quote_provider(&provider_id);
    let mut report = RefreshReport { source: provider_id.clone(), ..Default::default() };

    let exchanges: HashMap<i32, exchange::Model> = exchange::Entity::find()
        .all(&state.db)
        .await?
        .into_iter()
        .map(|e| (e.id, e))
        .collect();

    let mut query = instrument::Entity::find()
        .filter(instrument::Column::IsActive.eq(true))
        // Funds price off AMFI, not off a stock quote feed.
        .filter(instrument::Column::Kind.ne("MUTUAL_FUND"));
    if let Some(ids) = &only {
        query = query.filter(instrument::Column::Id.is_in(ids.clone()));
    }
    let instruments = query.all(&state.db).await?;

    // Two instruments can share a provider symbol (same ETF in two accounts
    // is one instrument, but a dual listing is two rows) - de-duplicate the
    // network calls, then fan the result back out.
    let mut symbol_to_instruments: HashMap<String, Vec<instrument::Model>> = HashMap::new();
    for inst in instruments {
        let suffix = inst
            .exchange_id
            .and_then(|id| exchanges.get(&id))
            .and_then(|e| e.quote_suffix.clone());
        let symbol = inst
            .quote_symbol
            .clone()
            .unwrap_or_else(|| compose_symbol(&inst.symbol, suffix.as_deref()));
        symbol_to_instruments.entry(symbol).or_default().push(inst);
    }

    report.requested = symbol_to_instruments.len();
    if symbol_to_instruments.is_empty() {
        return Ok(report);
    }

    let symbols: Vec<String> = symbol_to_instruments.keys().cloned().collect();
    let quotes = match provider.quotes(state, &symbols).await {
        Ok(q) => q,
        Err(e) => {
            report.warnings.push(format!("provider failed: {e}"));
            return Ok(report);
        }
    };

    let returned: HashSet<&str> = quotes.iter().map(|q| q.symbol.as_str()).collect();
    for symbol in &symbols {
        if !returned.contains(symbol.as_str()) {
            report.missing += 1;
            report.warnings.push(format!("no quote returned for {symbol}"));
        }
    }

    for q in quotes {
        let Some(targets) = symbol_to_instruments.get(&q.symbol) else { continue };
        for inst in targets {
            // Trust the instrument's configured currency over the provider's
            // when they disagree - the provider occasionally reports pence
            // for LSE lines, which would silently inflate values 100x.
            let currency = if q.currency.is_empty() {
                inst.currency_code.clone()
            } else {
                q.currency.clone()
            };
            if !currency.eq_ignore_ascii_case(&inst.currency_code) {
                report.warnings.push(format!(
                    "{}: provider quotes {} but instrument is {} - stored under the instrument's currency",
                    q.symbol, currency, inst.currency_code
                ));
            }
            upsert_price(&state.db, inst.id, q.as_of, q.price, &inst.currency_code, provider.id())
                .await?;
            report.updated += 1;
            if let Some(kind) = corrected_kind(&inst.kind, q.instrument_type.as_deref()) {
                set_kind(&state.db, inst, kind).await?;
                report.warnings.push(format!(
                    "{}: {} lists it as an {kind}, so it is now filed as one rather than as {}",
                    q.symbol,
                    provider.id(),
                    inst.kind
                ));
            }
        }
        report.as_of = Some(q.as_of);
    }

    Ok(report)
}

/// The kind an instrument should have, where the feed shows its current one
/// is wrong. A global import without an asset kind files everything as a
/// share, so an ETF such as SWRD or QQQ lands among the stocks with no
/// portfolio to open up. Only that one mistake is corrected: a kind the
/// user set to anything but the default is theirs.
pub fn corrected_kind(current: &str, provider_type: Option<&str>) -> Option<&'static str> {
    match provider_type {
        Some(t) if current == "EQUITY" && t.eq_ignore_ascii_case("ETF") => Some("ETF"),
        _ => None,
    }
}

pub async fn set_kind(db: &DatabaseConnection, inst: &instrument::Model, kind: &str) -> AppResult<()> {
    let mut m: instrument::ActiveModel = inst.clone().into();
    m.kind = Set(kind.to_string());
    m.updated_at = Set(chrono::Utc::now().naive_utc());
    m.update(db).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Indian mutual-fund NAV
// ---------------------------------------------------------------------------

pub async fn refresh_navs(state: &AppState) -> AppResult<RefreshReport> {
    let provider_id = state.setting("nav_provider", "amfi").await;
    let provider = providers::nav_provider(&provider_id);
    let mut report = RefreshReport { source: provider_id.clone(), ..Default::default() };

    let funds = instrument::Entity::find()
        .filter(instrument::Column::Kind.eq("MUTUAL_FUND"))
        .filter(instrument::Column::IsActive.eq(true))
        .all(&state.db)
        .await?;
    report.requested = funds.len();
    if funds.is_empty() {
        return Ok(report);
    }

    let navs = match provider.all_navs(state).await {
        Ok(n) => n,
        Err(e) => {
            report.warnings.push(format!("provider failed: {e}"));
            return Ok(report);
        }
    };

    // AMFI is keyed by scheme code; ISIN is the fallback for funds imported
    // from a statement that only carried an ISIN.
    let by_code: HashMap<&str, &providers::Nav> =
        navs.iter().map(|n| (n.scheme_code.as_str(), n)).collect();
    let by_isin: HashMap<&str, &providers::Nav> = navs
        .iter()
        .filter_map(|n| n.isin.as_deref().map(|i| (i, n)))
        .collect();

    for fund in funds {
        let by_identifier = fund
            .amfi_scheme_code
            .as_deref()
            .and_then(|c| by_code.get(c).copied())
            .or_else(|| fund.isin.as_deref().and_then(|i| by_isin.get(i).copied()))
            // A scheme imported without a code may still match by symbol.
            .or_else(|| by_code.get(fund.symbol.as_str()).copied());

        // Last resort, for statements that carry neither code nor ISIN
        // (Value Research's export among them): the scheme name itself. A
        // name match is a good guess, not a fact, so it is reported.
        let (matched, by_name) = match by_identifier {
            Some(nav) => (Some(nav), false),
            None => (scheme::match_by_name(&fund.name, &navs), true),
        };

        match matched {
            Some(nav) => {
                upsert_price(&state.db, fund.id, nav.as_of, nav.nav, "INR", provider.id()).await?;
                remember_scheme_facts(&state.db, &fund, nav).await?;
                if by_name {
                    report.warnings.push(format!(
                        "'{}' matched AMFI scheme {} ({}) by name - check it is the right one",
                        fund.name, nav.scheme_code, nav.scheme_name
                    ));
                }
                report.updated += 1;
                report.as_of = Some(nav.as_of);
            }
            None => {
                report.missing += 1;
                report.warnings.push(format!(
                    "no NAV match for '{}' (set its AMFI scheme code to fix)",
                    fund.name
                ));
            }
        }
    }

    Ok(report)
}

/// Write back what the NAV row taught us about the instrument: the scheme
/// code and ISIN when the import had neither, and the AMFI category that
/// decides whether the fund is equity or debt. Once the code is stored the
/// next refresh matches on it directly, and the history backfill - which
/// needs a code - can run.
async fn remember_scheme_facts(
    db: &DatabaseConnection,
    fund: &instrument::Model,
    nav: &providers::Nav,
) -> AppResult<()> {
    let learn_code = fund.amfi_scheme_code.is_none();
    let learn_isin = fund.isin.is_none() && nav.isin.is_some();
    let learn_category = nav
        .category
        .as_deref()
        .is_some_and(|c| fund.sector.as_deref() != Some(c));

    if !learn_code && !learn_isin && !learn_category {
        return Ok(());
    }

    let mut m: instrument::ActiveModel = fund.clone().into();
    if learn_code {
        m.amfi_scheme_code = Set(Some(nav.scheme_code.clone()));
    }
    if learn_isin {
        m.isin = Set(nav.isin.clone());
    }
    if learn_category {
        m.sector = Set(nav.category.clone());
    }
    m.updated_at = Set(chrono::Utc::now().naive_utc());
    m.update(db).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Historical NAV backfill
// ---------------------------------------------------------------------------

/// How far back a fund's chart reaches: ten years of daily NAV, or further
/// when the first purchase is older than that.
pub const NAV_HISTORY_YEARS: i32 = 10;

/// Price sources the NAV feeds write, as opposed to the importer (`import`)
/// and the manual-price endpoint (`manual`). What a scheme-code change has
/// to throw away.
pub const NAV_FEED_SOURCES: &[&str] = &["amfi", "mfapi_history"];

/// Backfill each mutual fund's daily NAV over the last `NAV_HISTORY_YEARS`
/// years - or back to its first transaction, when that is older - so the
/// chart is the fund's actual history with the purchases marked on it, and
/// the period returns (1d/1w/1m/3m/...) have something to measure against.
///
/// `instrument.history_from` records how far back a fund has already been
/// filled, and the fund is skipped while that reaches `earliest_needed`.
/// The stored rows cannot decide this on their own: a fund launched six
/// years ago has no ten-year history to reach, and would be fetched again
/// every night. The marker only moves back when it has to (an older
/// transaction turns up) and is cleared when the scheme code changes.
/// Gated by the `history_backfill_enabled` setting.
pub async fn backfill_nav_history(state: &AppState) -> AppResult<RefreshReport> {
    let mut report = RefreshReport { source: "mfapi_history".to_string(), ..Default::default() };

    if state.setting("history_backfill_enabled", "true").await != "true" {
        return Ok(report);
    }

    let funds = instrument::Entity::find()
        .filter(instrument::Column::Kind.eq("MUTUAL_FUND"))
        .filter(instrument::Column::IsActive.eq(true))
        .filter(instrument::Column::AmfiSchemeCode.is_not_null())
        .all(&state.db)
        .await?;
    report.requested = funds.len();

    let window_start = history_window_start(chrono::Utc::now().date_naive());

    for fund in funds {
        let Some(code) = fund.amfi_scheme_code.clone() else { continue };

        let first_trade = txn::Entity::find()
            .filter(txn::Column::InstrumentId.eq(fund.id))
            .order_by_asc(txn::Column::TradeDate)
            .one(&state.db)
            .await?
            .map(|t| t.trade_date);
        let Some(first_trade) = first_trade else { continue };
        let earliest_needed = first_trade.min(window_start);

        if fund.history_from.is_some_and(|from| from <= earliest_needed) {
            continue;
        }

        match nav_history_mfapi::history(state, &code).await {
            Ok(navs) => {
                if navs.is_empty() {
                    report.missing += 1;
                    report.warnings.push(format!(
                        "{}: mfapi has no history for scheme {code} - check the code",
                        fund.name
                    ));
                    continue;
                }
                let points: Vec<providers::DailyClose> = navs
                    .iter()
                    .map(|n| providers::DailyClose { as_of: n.as_of, close: n.nav })
                    .collect();
                let inserted = insert_missing_prices(
                    &state.db,
                    fund.id,
                    &points,
                    earliest_needed,
                    "INR",
                    "mfapi_history",
                )
                .await?;

                let mut m: instrument::ActiveModel = fund.clone().into();
                m.history_from = Set(Some(earliest_needed));
                // mfapi names the category too - fill it in for a fund the
                // AMFI file did not classify (older layouts have no headings).
                if fund.sector.is_none() {
                    if let Some(cat) = navs.iter().find_map(|n| n.category.clone()) {
                        m.sector = Set(Some(cat));
                    }
                }
                m.updated_at = Set(chrono::Utc::now().naive_utc());
                m.update(&state.db).await?;

                report.updated += inserted;
                if let Some(last) = points.last() {
                    report.as_of = Some(report.as_of.map_or(last.as_of, |d| d.max(last.as_of)));
                }
            }
            Err(e) => {
                report.missing += 1;
                report.warnings.push(format!("{}: {e}", fund.name));
            }
        }
    }

    Ok(report)
}

/// `NAV_HISTORY_YEARS` before `today`; a leap day lands on 28 February.
fn history_window_start(today: NaiveDate) -> NaiveDate {
    let year = today.year() - NAV_HISTORY_YEARS;
    NaiveDate::from_ymd_opt(year, today.month(), today.day())
        .or_else(|| NaiveDate::from_ymd_opt(year, today.month(), 28))
        .expect("every month has a 28th")
}

/// The same backfill for everything that is *not* a fund - equities, ETFs,
/// anything priced off the quote feed.
///
/// Yahoo's chart endpoint already carries the full daily series; the quote
/// path only ever asked it for the last five days. Without this an equity
/// row expands to a chart with a single dot, because the only closes stored
/// are the ones this app happened to be running for.
///
/// Skipped once feed history already reaches the first transaction, so it
/// is a one-off cost per instrument rather than a fetch on every refresh,
/// and gated by the same `history_backfill_enabled` setting as the fund
/// backfill.
pub async fn backfill_quote_history(state: &AppState) -> AppResult<RefreshReport> {
    let mut report = RefreshReport { source: "yahoo_history".to_string(), ..Default::default() };

    if state.setting("history_backfill_enabled", "true").await != "true" {
        return Ok(report);
    }
    // Only Yahoo's provider exposes a history endpoint; a different quote
    // vendor keeps the daily snapshots and skips the backfill.
    if state.setting("quote_provider", "yahoo").await != "yahoo" {
        return Ok(report);
    }

    let exchanges: HashMap<i32, exchange::Model> = exchange::Entity::find()
        .all(&state.db)
        .await?
        .into_iter()
        .map(|e| (e.id, e))
        .collect();

    let instruments = instrument::Entity::find()
        .filter(instrument::Column::IsActive.eq(true))
        .filter(instrument::Column::Kind.ne("MUTUAL_FUND"))
        .all(&state.db)
        .await?;

    for inst in instruments {
        let earliest_needed = txn::Entity::find()
            .filter(txn::Column::InstrumentId.eq(inst.id))
            .order_by_asc(txn::Column::TradeDate)
            .one(&state.db)
            .await?
            .map(|t| t.trade_date);
        let Some(earliest_needed) = earliest_needed else { continue };

        if history_reaches(&state.db, inst.id, earliest_needed).await? {
            continue;
        }

        let suffix = inst
            .exchange_id
            .and_then(|id| exchanges.get(&id))
            .and_then(|e| e.quote_suffix.clone());
        let symbol = inst
            .quote_symbol
            .clone()
            .unwrap_or_else(|| compose_symbol(&inst.symbol, suffix.as_deref()));

        report.requested += 1;
        match quotes_yahoo::history(state, &symbol).await {
            Ok(points) => {
                if points.is_empty() {
                    report.missing += 1;
                    report.warnings.push(format!("{symbol}: no history returned"));
                    continue;
                }
                let inserted = insert_missing_prices(
                    &state.db,
                    inst.id,
                    &points,
                    earliest_needed,
                    &inst.currency_code,
                    "yahoo_history",
                )
                .await?;
                report.updated += inserted;
                if let Some(last) = points.last() {
                    report.as_of = Some(report.as_of.map_or(last.as_of, |d| d.max(last.as_of)));
                }
            }
            Err(e) => {
                report.missing += 1;
                report.warnings.push(format!("{symbol}: {e}"));
            }
        }
    }

    Ok(report)
}

/// Price sources that are a single day's snapshot rather than a series:
/// the importer's transaction-date NAV, a hand-entered price, and the daily
/// quote/NAV refresh. Only a *history* source proves the series is filled.
const SNAPSHOT_SOURCES: &[&str] = &["import", "manual", "amfi", "yahoo"];

/// True when a history feed has already filled `price_point` back to
/// `earliest_needed` for this instrument.
async fn history_reaches(
    db: &DatabaseConnection,
    instrument_id: i32,
    earliest_needed: NaiveDate,
) -> AppResult<bool> {
    let earliest_history = price_point::Entity::find()
        .filter(price_point::Column::InstrumentId.eq(instrument_id))
        .filter(price_point::Column::Source.is_not_in(SNAPSHOT_SOURCES.iter().copied()))
        .order_by_asc(price_point::Column::AsOf)
        .one(db)
        .await?
        .map(|p| p.as_of);
    Ok(earliest_history.is_some_and(|d| d <= earliest_needed))
}

/// Insert every `(instrument, date)` from `points` that isn't already in
/// `price_point`, dated on or after `earliest_needed`. Done as one bulk
/// insert rather than the row-at-a-time `upsert_price` - a fund's full
/// history can run into the thousands of rows.
async fn insert_missing_prices(
    db: &DatabaseConnection,
    instrument_id: i32,
    points: &[providers::DailyClose],
    earliest_needed: NaiveDate,
    currency: &str,
    source: &str,
) -> AppResult<usize> {
    let existing: HashSet<NaiveDate> = price_point::Entity::find()
        .filter(price_point::Column::InstrumentId.eq(instrument_id))
        .all(db)
        .await?
        .into_iter()
        .map(|p| p.as_of)
        .collect();

    let now = chrono::Utc::now().naive_utc();
    let rows: Vec<price_point::ActiveModel> = points
        .iter()
        .filter(|p| p.as_of >= earliest_needed && !existing.contains(&p.as_of))
        .map(|p| price_point::ActiveModel {
            instrument_id: Set(instrument_id),
            as_of: Set(p.as_of),
            close: Set(p.close),
            currency_code: Set(currency.to_string()),
            source: Set(source.to_string()),
            created_at: Set(now),
            ..Default::default()
        })
        .collect();

    let count = rows.len();
    // SQLite caps the bound parameters in one statement (32,766 by default,
    // 999 on older builds); a fund with twenty years of daily NAV would blow
    // through that in a single insert. Batches keep every backend happy.
    let mut pending = rows.into_iter().peekable();
    while pending.peek().is_some() {
        let batch: Vec<price_point::ActiveModel> = pending.by_ref().take(INSERT_BATCH).collect();
        price_point::Entity::insert_many(batch).exec(db).await?;
    }
    Ok(count)
}

/// Rows per bulk insert: 6 columns each, comfortably under any parameter cap.
const INSERT_BATCH: usize = 150;

/// Put a refresh's outcome in the log, warnings included, so a feed that
/// quietly returns nothing for one fund is visible without opening the UI.
pub fn log_reports(reports: &[RefreshReport]) {
    for r in reports {
        tracing::info!(
            source = %r.source,
            requested = r.requested,
            updated = r.updated,
            missing = r.missing,
            "refresh finished"
        );
        for w in &r.warnings {
            tracing::warn!(source = %r.source, "{w}");
        }
    }
}

// ---------------------------------------------------------------------------
// FX
// ---------------------------------------------------------------------------

pub async fn refresh_fx(state: &AppState) -> AppResult<RefreshReport> {
    let provider_id = state.setting("fx_provider", "frankfurter").await;
    let provider = providers::fx_provider(&provider_id);
    let mut report = RefreshReport { source: provider_id.clone(), ..Default::default() };

    let pairs = pairs_to_refresh(state).await?;
    report.requested = pairs.len();
    if pairs.is_empty() {
        return Ok(report);
    }

    // Group by base so each base costs one request, not one per pair.
    let mut by_base: HashMap<String, Vec<String>> = HashMap::new();
    for (base, quote) in pairs {
        by_base.entry(base).or_default().push(quote);
    }

    for (base, quotes) in by_base {
        match provider.rates(state, &base, &quotes).await {
            Ok(rates) => {
                for r in rates {
                    upsert_fx(&state.db, &r.base, &r.quote, r.rate, r.as_of, provider.id()).await?;
                    report.updated += 1;
                    report.as_of = Some(r.as_of);
                }
            }
            Err(e) => {
                report.missing += quotes.len();
                report.warnings.push(format!("{base}: {e}"));
            }
        }
    }

    // After the rates: the fee samples are sized from them.
    match refresh_fx_fees(state).await {
        Ok((updated, warnings)) => {
            report.fees_updated = updated;
            report.warnings.extend(warnings);
        }
        Err(e) => report.warnings.push(format!("Wise fees: {e}")),
    }

    Ok(report)
}

// ---------------------------------------------------------------------------
// FX fees
// ---------------------------------------------------------------------------

/// Fees are re-sampled at most this often. Wise changes its pricing rarely,
/// and each pair costs one request per sample.
const FEES_REFETCH_HOURS: i64 = 12;

/// Where each pair's fee curve is sampled, in US dollars' worth of the
/// currency being converted. Wise's volume discounts start near 20-25k USD
/// and step down again at a few hundred thousand and a million; samples
/// either side of those keep the interpolation close.
const FEE_SAMPLES_USD: &[f64] = &[
    1_000.0, 20_000.0, 30_000.0, 50_000.0, 100_000.0, 250_000.0, 500_000.0, 1_000_000.0,
    2_500_000.0, 5_000_000.0,
];

/// Sample Wise's fees for every pair any user has set to `WISE`. A pair a
/// user's totals need but they have no setting for yet starts on `WISE`, so
/// a newly held currency is priced like the rest. Fee schedules are market
/// data, fetched once per pair however many users price with them. Returns
/// how many pairs were updated and what went wrong for the others.
async fn refresh_fx_fees(state: &AppState) -> AppResult<(usize, Vec<String>)> {
    let now = chrono::Utc::now().naive_utc();

    let mut settings: HashMap<(i32, String, String), fx_setting::Model> = fx_setting::Entity::find()
        .all(&state.db)
        .await?
        .into_iter()
        .map(|s| ((s.user_id, s.base_code.to_uppercase(), s.quote_code.to_uppercase()), s))
        .collect();
    let held = held_currencies(state).await?;
    for (user, base) in user_bases(state).await? {
        for ccy in held.iter().filter(|c| **c != base) {
            let key = (user, ccy.clone(), base.clone());
            if settings.contains_key(&key) {
                continue;
            }
            let created = fx_setting::ActiveModel {
                user_id: Set(user),
                base_code: Set(ccy.clone()),
                quote_code: Set(base.clone()),
                provider: Set("default".to_string()),
                margin_mode: Set(MarginMode::Wise.to_string()),
                margin_value: Set(0.0),
                manual_rate: Set(None),
                use_manual: Set(false),
                note: Set(None),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(&state.db)
            .await?;
            settings.insert(key, created);
        }
    }
    let wise_pairs: HashSet<(String, String)> = settings
        .into_iter()
        .filter(|(_, s)| s.margin_mode == MarginMode::Wise.as_str() && !(s.use_manual && s.manual_rate.is_some()))
        .map(|((_, b, q), _)| (b, q))
        .collect();

    let stored: HashMap<(String, String), fx_fee::Model> = fx_fee::Entity::find()
        .all(&state.db)
        .await?
        .into_iter()
        .map(|f| ((f.base_code.clone(), f.quote_code.clone()), f))
        .collect();
    // Only raw rates are needed, to size the samples.
    let fx = FxEngine::load(&state.db, "USD", None).await?;

    // Pairs run side by side; each pair's samples go one after another.
    let mut jobs = tokio::task::JoinSet::new();
    for (b, q) in wise_pairs {
        let fresh = stored.get(&(b.clone(), q.clone())).is_some_and(|f| {
            f.route == fx_fees_wise::ROUTE && (now - f.fetched_at).num_hours() < FEES_REFETCH_HOURS
        });
        if fresh {
            continue;
        }
        let per_usd = fx.quote("USD", &b).map(|r| r.raw_rate).unwrap_or(1.0);
        let amounts: Vec<f64> = FEE_SAMPLES_USD.iter().map(|a| (a * per_usd).round().max(1.0)).collect();
        let state = state.clone();
        jobs.spawn(async move {
            let sampled = fx_fees_wise::sample(&state, &b, &q, &amounts).await;
            (b, q, sampled)
        });
    }

    let mut updated = 0;
    let mut warnings = Vec::new();
    while let Some(joined) = jobs.join_next().await {
        let Ok((b, q, sampled)) = joined else { continue };
        match sampled {
            Ok(s) => {
                upsert_fee(&state.db, &b, &q, &s, now).await?;
                updated += 1;
            }
            Err(e) => warnings.push(format!("Wise fees for {b}/{q}: {e}")),
        }
    }
    Ok((updated, warnings))
}

async fn upsert_fee(
    db: &DatabaseConnection,
    base: &str,
    quote: &str,
    sampled: &fx_fees_wise::Sampled,
    now: chrono::NaiveDateTime,
) -> AppResult<()> {
    let points = serde_json::to_string(&sampled.points).unwrap_or_else(|_| "[]".to_string());
    let existing = fx_fee::Entity::find()
        .filter(fx_fee::Column::BaseCode.eq(base))
        .filter(fx_fee::Column::QuoteCode.eq(quote))
        .one(db)
        .await?;
    let mut m: fx_fee::ActiveModel = match existing {
        Some(row) => row.into(),
        None => fx_fee::ActiveModel {
            base_code: Set(base.to_string()),
            quote_code: Set(quote.to_string()),
            ..Default::default()
        },
    };
    m.provider = Set(fx_fees_wise::PROVIDER.to_string());
    m.route = Set(fx_fees_wise::ROUTE.to_string());
    m.points = Set(points);
    m.mid_rate = Set(sampled.mid_rate);
    m.fetched_at = Set(now);
    m.save(db).await?;
    Ok(())
}

/// Every user and the currency they report in. With no users yet, the
/// default for new ones, so a fresh install still has rates to show.
async fn user_bases(state: &AppState) -> AppResult<Vec<(i32, String)>> {
    let mut out = Vec::new();
    for u in app_user::Entity::find()
        .filter(app_user::Column::Role.eq(Role::User.as_str()))
        .all(&state.db)
        .await?
    {
        out.push((u.id, prefs::base_currency(state, u.id).await));
    }
    Ok(out)
}

/// Every currency anything is held or quoted in, plus the display ones.
async fn held_currencies(state: &AppState) -> AppResult<HashSet<String>> {
    let mut held: HashSet<String> = HashSet::new();
    for i in instrument::Entity::find().all(&state.db).await? {
        held.insert(i.currency_code.to_uppercase());
    }
    for a in account::Entity::find().all(&state.db).await? {
        held.insert(a.currency_code.to_uppercase());
    }
    // Shown beside every headline figure, held or not.
    held.extend(DISPLAY_CURRENCIES.iter().map(|c| c.to_string()));
    Ok(held)
}

/// Which pairs are worth fetching: every pair any user has configured in
/// the Forex tab, plus every held currency into each base currency in use.
async fn pairs_to_refresh(state: &AppState) -> AppResult<Vec<(String, String)>> {
    let mut pairs: HashSet<(String, String)> = HashSet::new();

    for s in fx_setting::Entity::find().all(&state.db).await? {
        // A pair pinned to a manual rate needs no feed.
        if s.use_manual && s.manual_rate.is_some() {
            continue;
        }
        pairs.insert((s.base_code.to_uppercase(), s.quote_code.to_uppercase()));
    }

    let mut bases: HashSet<String> = user_bases(state).await?.into_iter().map(|(_, b)| b).collect();
    bases.insert(state.setting("default_base_currency", "SGD").await.to_uppercase());
    let held = held_currencies(state).await?;
    for base in &bases {
        for ccy in held.iter().filter(|c| *c != base) {
            pairs.insert((ccy.clone(), base.clone()));
        }
    }

    let mut out: Vec<(String, String)> = pairs.into_iter().collect();
    out.sort();
    Ok(out)
}

// ---------------------------------------------------------------------------
// Upserts
// ---------------------------------------------------------------------------

/// Write a price for (instrument, day), replacing any existing row.
///
/// Done as find-then-write rather than a native upsert because the two target
/// backends spell `ON CONFLICT` differently, and this path is nowhere near
/// hot enough for the extra round trip to matter.
pub async fn upsert_price(
    db: &DatabaseConnection,
    instrument_id: i32,
    as_of: NaiveDate,
    close: f64,
    currency: &str,
    source: &str,
) -> AppResult<()> {
    let existing = price_point::Entity::find()
        .filter(price_point::Column::InstrumentId.eq(instrument_id))
        .filter(price_point::Column::AsOf.eq(as_of))
        .one(db)
        .await?;

    match existing {
        Some(row) => {
            let mut m: price_point::ActiveModel = row.into();
            m.close = Set(close);
            m.currency_code = Set(currency.to_uppercase());
            m.source = Set(source.to_string());
            m.update(db).await?;
        }
        None => {
            price_point::ActiveModel {
                instrument_id: Set(instrument_id),
                as_of: Set(as_of),
                close: Set(close),
                currency_code: Set(currency.to_uppercase()),
                source: Set(source.to_string()),
                created_at: Set(chrono::Utc::now().naive_utc()),
                ..Default::default()
            }
            .insert(db)
            .await?;
        }
    }
    Ok(())
}

pub async fn upsert_fx(
    db: &DatabaseConnection,
    base: &str,
    quote: &str,
    rate: f64,
    as_of: NaiveDate,
    source: &str,
) -> AppResult<()> {
    let (base, quote) = (base.to_uppercase(), quote.to_uppercase());
    let existing = fx_rate::Entity::find()
        .filter(fx_rate::Column::BaseCode.eq(base.clone()))
        .filter(fx_rate::Column::QuoteCode.eq(quote.clone()))
        .filter(fx_rate::Column::AsOf.eq(as_of))
        .filter(fx_rate::Column::Source.eq(source))
        .one(db)
        .await?;

    match existing {
        Some(row) => {
            let mut m: fx_rate::ActiveModel = row.into();
            m.rate = Set(rate);
            m.update(db).await?;
        }
        None => {
            fx_rate::ActiveModel {
                base_code: Set(base),
                quote_code: Set(quote),
                rate: Set(rate),
                as_of: Set(as_of),
                source: Set(source.to_string()),
                created_at: Set(chrono::Utc::now().naive_utc()),
                ..Default::default()
            }
            .insert(db)
            .await?;
        }
    }
    Ok(())
}

/// Refresh everything, for the dashboard's single "Refresh" button and for
/// the scheduled job.
pub async fn refresh_all(state: &AppState) -> AppResult<Vec<RefreshReport>> {
    let reports = vec![
        refresh_fx(state).await?,
        refresh_quotes(state, None).await?,
        refresh_navs(state).await?,
        backfill_nav_history(state).await?,
        backfill_quote_history(state).await?,
        super::lookthrough::refresh_fund_holdings(state, super::portfolio::Scope::IndiaMf, None, false).await?,
        super::lookthrough::refresh_fund_holdings(state, super::portfolio::Scope::Global, None, false).await?,
    ];
    log_reports(&reports);
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn only_a_share_the_feed_calls_an_etf_is_refiled() {
        assert_eq!(corrected_kind("EQUITY", Some("ETF")), Some("ETF"));
        assert_eq!(corrected_kind("EQUITY", Some("etf")), Some("ETF"));
        assert_eq!(corrected_kind("EQUITY", Some("EQUITY")), None);
        assert_eq!(corrected_kind("EQUITY", None), None);
        assert_eq!(corrected_kind("REIT", Some("ETF")), None);
        assert_eq!(corrected_kind("ETF", Some("EQUITY")), None);
    }

    #[test]
    fn the_history_window_is_ten_calendar_years() {
        assert_eq!(history_window_start(d(2026, 9, 22)), d(2016, 9, 22));
        // 29 Feb 2024 has no counterpart in 2014.
        assert_eq!(history_window_start(d(2024, 2, 29)), d(2014, 2, 28));
    }
}
