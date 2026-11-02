//! Free FX reference rates (ECB-sourced) via frankfurter.app.
//!
//! The default provider: no key, no rate limit worth worrying about, and
//! daily mid-market rates that are more than good enough for valuing a
//! portfolio. Use Xe instead when you need intraday or a contractual source.

use async_trait::async_trait;
use chrono::NaiveDate;
use serde::Deserialize;
use std::collections::HashMap;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

use super::{FxProvider, FxQuote};

const DEFAULT_BASE: &str = "https://api.frankfurter.app";
const PROVIDER: &str = "frankfurter";

pub struct FrankfurterProvider;

#[derive(Deserialize)]
struct LatestResponse {
    date: NaiveDate,
    rates: HashMap<String, f64>,
}

#[async_trait]
impl FxProvider for FrankfurterProvider {
    fn id(&self) -> &'static str {
        PROVIDER
    }

    fn label(&self) -> &'static str {
        "Frankfurter (ECB, free)"
    }

    async fn rates(&self, state: &AppState, base: &str, quotes: &[String]) -> AppResult<Vec<FxQuote>> {
        if quotes.is_empty() {
            return Ok(Vec::new());
        }
        let root = state
            .setting_opt("frankfurter_base_url")
            .await
            .unwrap_or_else(|| DEFAULT_BASE.to_string());

        let symbols = quotes.join(",");
        let res = state
            .http
            .get(format!("{}/latest", root.trim_end_matches('/')))
            .query(&[("from", base), ("to", symbols.as_str())])
            .send()
            .await?;

        if !res.status().is_success() {
            return Err(AppError::provider(PROVIDER, format!("HTTP {}", res.status())));
        }

        let body: LatestResponse = res.json().await?;
        Ok(body
            .rates
            .into_iter()
            .map(|(quote, rate)| FxQuote {
                base: base.to_uppercase(),
                quote: quote.to_uppercase(),
                rate,
                as_of: body.date,
            })
            .collect())
    }
}
