//! Price-based performance over configurable lookback periods (1d/1w/1m/...).
//!
//! This is a plain price return - "what would $1 in this fund be worth
//! N days ago be worth today" - not the money-weighted XIRR in
//! `domain::holdings`, which already exists and accounts for the timing and
//! size of each cash flow. It exists because that XIRR only ever needed the
//! *latest* price; showing "how has this done over the last month" needs a
//! price from a month ago too, which `price_point` now actually has thanks
//! to import capture and the mfapi.in backfill in `services::refresh`.

use std::collections::HashMap;

use chrono::{Duration, NaiveDate};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use serde::Serialize;

use crate::domain::money::pct_change;
use crate::entities::{instrument, price_point, txn};
use crate::error::AppResult;
use crate::services::{prefs, users};
use crate::state::AppState;

const DEFAULT_PERIODS: &str = "1d,1w,1m,3m,6m,1y";

/// Parse a comma-separated period list like "1d,5d,1m,3m,6m,1y", in order,
/// deduplicated. Months are approximated as 30 days and years as 365 - good
/// enough for "roughly how has this done", not for tax-grade calculations.
pub fn parse_periods(spec: &str) -> Result<Vec<(String, i64)>, String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for token in spec.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let days = period_days(token)?;
        if seen.insert(token.to_string()) {
            out.push((token.to_string(), days));
        }
    }
    if out.is_empty() {
        return Err("performance_periods must list at least one period, e.g. '1d,1m,1y'".to_string());
    }
    Ok(out)
}

fn period_days(token: &str) -> Result<i64, String> {
    if token.len() < 2 {
        return Err(format!("invalid period '{token}': expected e.g. '1d', '3m', '1y'"));
    }
    let (num, unit) = token.split_at(token.len() - 1);
    let n: i64 = num.parse().map_err(|_| format!("invalid period '{token}'"))?;
    if n <= 0 {
        return Err(format!("invalid period '{token}': must be positive"));
    }
    match unit {
        "d" => Ok(n),
        "w" => Ok(n * 7),
        "m" => Ok(n * 30),
        "y" => Ok(n * 365),
        _ => Err(format!("invalid period '{token}': unit must be d, w, m or y")),
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct InstrumentPerformance {
    pub instrument_id: i32,
    pub symbol: String,
    pub name: String,
    /// Period label -> percent change, or null when there isn't enough
    /// stored history to reach that far back.
    pub returns: HashMap<String, Option<f64>>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PerformanceReport {
    /// The configured periods, in order - so the frontend can render
    /// columns in a stable order without re-deriving it.
    pub periods: Vec<String>,
    pub instruments: Vec<InstrumentPerformance>,
}

/// Price-only return for each of the user's periods, for every active
/// instrument they have dealt in. One with no stored price yet simply comes back with every
/// period `null`.
pub async fn compute(state: &AppState, owner: i32) -> AppResult<PerformanceReport> {
    let spec = prefs::get(&state.db, owner, "performance_periods", DEFAULT_PERIODS).await;
    let periods = parse_periods(&spec).unwrap_or_else(|_| {
        parse_periods(DEFAULT_PERIODS).expect("DEFAULT_PERIODS is a valid period list")
    });

    let held: Vec<i32> = txn::Entity::find()
        .filter(txn::Column::AccountId.is_in(users::account_ids(&state.db, owner).await?))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|t| t.instrument_id)
        .collect();
    let instruments = instrument::Entity::find()
        .filter(instrument::Column::IsActive.eq(true))
        .filter(instrument::Column::Id.is_in(held))
        .all(&state.db)
        .await?;

    let mut out = Vec::with_capacity(instruments.len());
    for inst in instruments {
        let history = price_point::Entity::find()
            .filter(price_point::Column::InstrumentId.eq(inst.id))
            .order_by_asc(price_point::Column::AsOf)
            .all(&state.db)
            .await?;

        let mut returns = HashMap::with_capacity(periods.len());
        if let Some(latest) = history.last() {
            for (label, days) in &periods {
                let target_date = latest.as_of - Duration::days(*days);
                let target_price = price_on_or_before(&history, target_date);
                returns.insert(label.clone(), target_price.and_then(|p| pct_change(p, latest.close)));
            }
        } else {
            for (label, _) in &periods {
                returns.insert(label.clone(), None);
            }
        }

        out.push(InstrumentPerformance {
            instrument_id: inst.id,
            symbol: inst.symbol,
            name: inst.name,
            returns,
        });
    }

    Ok(PerformanceReport { periods: periods.into_iter().map(|(label, _)| label).collect(), instruments: out })
}

/// The most recent price on or before `date`. History is sparse - only
/// trading days, plus whatever backfill managed to fetch - so requiring an
/// exact match on `date` would miss almost every lookup.
fn price_on_or_before(history: &[price_point::Model], date: NaiveDate) -> Option<f64> {
    history.iter().rev().find(|p| p.as_of <= date).map(|p| p.close)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_mixed_unit_list_and_dedupes() {
        let periods = parse_periods("1d, 1w,1m,1m,3m,1y").unwrap();
        assert_eq!(
            periods,
            vec![
                ("1d".to_string(), 1),
                ("1w".to_string(), 7),
                ("1m".to_string(), 30),
                ("3m".to_string(), 90),
                ("1y".to_string(), 365),
            ]
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_periods("").is_err());
        assert!(parse_periods("banana").is_err());
        assert!(parse_periods("0d").is_err());
        assert!(parse_periods("1x").is_err());
    }

    #[test]
    fn finds_the_closest_prior_trading_day() {
        let history = vec![
            point(2026, 1, 3, 100.0),
            point(2026, 1, 6, 102.0),
            point(2026, 1, 10, 105.0),
        ];
        assert_eq!(price_on_or_before(&history, NaiveDate::from_ymd_opt(2026, 1, 8).unwrap()), Some(102.0));
        assert_eq!(price_on_or_before(&history, NaiveDate::from_ymd_opt(2025, 12, 1).unwrap()), None);
    }

    fn point(y: i32, m: u32, d: u32, close: f64) -> price_point::Model {
        price_point::Model {
            id: 0,
            instrument_id: 1,
            as_of: NaiveDate::from_ymd_opt(y, m, d).unwrap(),
            close,
            currency_code: "INR".to_string(),
            source: "test".to_string(),
            created_at: chrono::Utc::now().naive_utc(),
        }
    }
}
