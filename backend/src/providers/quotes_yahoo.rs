//! Equity / ETF quotes.
//!
//! Uses Yahoo Finance's chart endpoint, which is free and covers NSE, BSE,
//! ASX, SGX and the US markets in one place. It is an *unofficial* API: it
//! can rate-limit or change shape without notice. Two mitigations are built
//! in - `quote_base_url` in Settings repoints this at your own proxy, and a
//! failed fetch always leaves the last stored price untouched.

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;

use crate::entities::{exchange, instrument};
use crate::error::AppResult;
use crate::state::AppState;

use super::{DailyClose, Quote, QuoteProvider};

const DEFAULT_BASE: &str = "https://query1.finance.yahoo.com/v8/finance/chart";
const PROVIDER: &str = "yahoo";

pub struct YahooProvider;

#[derive(Deserialize)]
struct ChartEnvelope {
    chart: Chart,
}

#[derive(Deserialize)]
struct Chart {
    result: Option<Vec<ChartResult>>,
}

#[derive(Deserialize)]
struct ChartResult {
    meta: Meta,
    /// Only present when a history range is requested; the quote path
    /// reads `meta` alone.
    #[serde(default)]
    timestamp: Vec<i64>,
    #[serde(default)]
    indicators: Indicators,
}

#[derive(Default, Deserialize)]
struct Indicators {
    #[serde(default)]
    quote: Vec<QuoteSeries>,
}

#[derive(Default, Deserialize)]
struct QuoteSeries {
    /// One entry per timestamp; `null` on a day the exchange was shut or
    /// the bar is simply missing.
    #[serde(default)]
    close: Vec<Option<f64>>,
}

#[derive(Deserialize)]
struct Meta {
    #[serde(rename = "regularMarketPrice")]
    regular_market_price: Option<f64>,
    #[serde(rename = "previousClose")]
    previous_close: Option<f64>,
    currency: Option<String>,
    #[serde(rename = "regularMarketTime")]
    regular_market_time: Option<i64>,
    /// `EQUITY`, `ETF`, `MUTUALFUND`, ...
    #[serde(rename = "instrumentType")]
    instrument_type: Option<String>,
}

#[async_trait]
impl QuoteProvider for YahooProvider {
    fn id(&self) -> &'static str {
        PROVIDER
    }

    async fn quotes(&self, state: &AppState, symbols: &[String]) -> AppResult<Vec<Quote>> {
        let base = state
            .setting_opt("quote_base_url")
            .await
            .unwrap_or_else(|| DEFAULT_BASE.to_string());

        let mut out = Vec::with_capacity(symbols.len());
        for symbol in symbols {
            // One symbol at a time, and one failure never blocks the rest -
            // a delisted ticker in a 40-line portfolio must not blank the
            // other 39.
            match fetch_one(state, &base, symbol).await {
                Ok(Some(q)) => out.push(q),
                Ok(None) => tracing::warn!(symbol, "no price in quote response"),
                Err(e) => tracing::warn!(symbol, error = %e, "quote fetch failed"),
            }
        }
        Ok(out)
    }
}

async fn fetch_one(state: &AppState, base: &str, symbol: &str) -> AppResult<Option<Quote>> {
    let url = format!("{}/{}", base.trim_end_matches('/'), urlencode(symbol));
    let res = state
        .http
        .get(&url)
        .query(&[("interval", "1d"), ("range", "5d")])
        .send()
        .await?;

    if !res.status().is_success() {
        tracing::warn!(symbol, status = %res.status(), "quote endpoint returned non-success");
        return Ok(None);
    }

    let body: ChartEnvelope = res.json().await?;
    let Some(result) = body.chart.result.and_then(|r| r.into_iter().next()) else {
        return Ok(None);
    };

    // Outside market hours `regularMarketPrice` can be absent; the previous
    // close is the honest answer then.
    let Some(price) = result.meta.regular_market_price.or(result.meta.previous_close) else {
        return Ok(None);
    };

    Ok(Some(Quote {
        symbol: symbol.to_string(),
        price,
        currency: result.meta.currency.unwrap_or_default().to_uppercase(),
        as_of: epoch_to_date(result.meta.regular_market_time),
        instrument_type: result.meta.instrument_type,
    }))
}

/// Full daily close history for one provider symbol, oldest first.
///
/// The same chart endpoint as `fetch_one`, asked for a range instead of a
/// snapshot - so an equity gets the same "what was this worth a month ago"
/// depth that `nav_history_mfapi` gives a fund. A missing bar (exchange
/// holiday, or Yahoo simply has a hole) is skipped rather than failing the
/// symbol: one absent day should not cost the other three thousand.
pub async fn history(state: &AppState, symbol: &str) -> AppResult<Vec<DailyClose>> {
    let base = state
        .setting_opt("quote_base_url")
        .await
        .unwrap_or_else(|| DEFAULT_BASE.to_string());

    let url = format!("{}/{}", base.trim_end_matches('/'), urlencode(symbol));
    let res = state
        .http
        .get(&url)
        .query(&[("interval", "1d"), ("range", "max")])
        .send()
        .await?;

    if !res.status().is_success() {
        return Err(crate::error::AppError::provider(
            PROVIDER,
            format!("HTTP {} for {symbol}", res.status()),
        ));
    }

    let body: ChartEnvelope = res.json().await?;
    let Some(result) = body.chart.result.and_then(|r| r.into_iter().next()) else {
        return Ok(Vec::new());
    };
    let Some(series) = result.indicators.quote.into_iter().next() else {
        return Ok(Vec::new());
    };

    let mut out = Vec::with_capacity(result.timestamp.len());
    for (epoch, close) in result.timestamp.iter().zip(series.close) {
        let Some(close) = close else { continue };
        if !close.is_finite() || close <= 0.0 {
            continue;
        }
        let Some(as_of) = DateTime::<Utc>::from_timestamp(*epoch, 0).map(|dt| dt.date_naive())
        else {
            continue;
        };
        out.push(DailyClose { as_of, close });
    }

    out.sort_by_key(|p| p.as_of);
    out.dedup_by_key(|p| p.as_of);
    Ok(out)
}

fn epoch_to_date(epoch: Option<i64>) -> NaiveDate {
    epoch
        .and_then(|t| DateTime::<Utc>::from_timestamp(t, 0))
        .map(|dt| dt.date_naive())
        .unwrap_or_else(|| Utc::now().date_naive())
}

/// Percent-encode the few characters that actually appear in tickers
/// (`^` for indices, `=` for FX pairs, spaces in malformed input).
pub(crate) fn urlencode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}

/// Build the provider symbol for an instrument: ticker plus the exchange's
/// suffix, e.g. `INFY` on NSE becomes `INFY.NS`. US tickers take no suffix.
pub fn compose_symbol(symbol: &str, exchange_suffix: Option<&str>) -> String {
    match exchange_suffix {
        Some(sfx) if !sfx.is_empty() && !symbol.ends_with(sfx) => format!("{symbol}{sfx}"),
        _ => symbol.to_string(),
    }
}

/// The symbol Yahoo knows an instrument by: its own quote symbol when one
/// is set, otherwise the ticker with its exchange's suffix.
pub fn provider_symbol(inst: &instrument::Model, exchanges: &HashMap<i32, exchange::Model>) -> String {
    if let Some(q) = inst.quote_symbol.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        return q.to_string();
    }
    let suffix = inst
        .exchange_id
        .and_then(|id| exchanges.get(&id))
        .and_then(|e| e.quote_suffix.as_deref());
    compose_symbol(&inst.symbol, suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composes_exchange_suffixes() {
        assert_eq!(compose_symbol("INFY", Some(".NS")), "INFY.NS");
        assert_eq!(compose_symbol("BHP", Some(".AX")), "BHP.AX");
        assert_eq!(compose_symbol("VOO", None), "VOO");
        assert_eq!(compose_symbol("VOO", Some("")), "VOO");
    }

    #[test]
    fn does_not_double_apply_a_suffix() {
        assert_eq!(compose_symbol("INFY.NS", Some(".NS")), "INFY.NS");
    }

    #[test]
    fn encodes_index_and_fx_tickers() {
        assert_eq!(urlencode("^NSEI"), "%5ENSEI");
        assert_eq!(urlencode("USDINR=X"), "USDINR%3DX");
        assert_eq!(urlencode("INFY.NS"), "INFY.NS");
    }
}
