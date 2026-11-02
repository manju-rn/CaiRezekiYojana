//! FX rates via exchangerate.host.
//!
//! A second free-tier option, kept because it covers a few currencies the ECB
//! feed does not. Newer plans require an access key, so it is treated as
//! unconfigured until `exchangerate_host_key` is set - except on legacy
//! keyless accounts, where an empty key still works.

use async_trait::async_trait;
use chrono::NaiveDate;
use serde::Deserialize;
use std::collections::HashMap;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

use super::{FxProvider, FxQuote};

const DEFAULT_BASE: &str = "https://api.exchangerate.host";
const PROVIDER: &str = "exchangerate_host";

pub struct ExchangeRateHostProvider;

#[derive(Deserialize)]
struct LiveResponse {
    #[serde(default)]
    success: Option<bool>,
    #[serde(default)]
    date: Option<NaiveDate>,
    #[serde(default)]
    quotes: HashMap<String, f64>,
    #[serde(default)]
    rates: HashMap<String, f64>,
}

#[async_trait]
impl FxProvider for ExchangeRateHostProvider {
    fn id(&self) -> &'static str {
        PROVIDER
    }

    fn label(&self) -> &'static str {
        "exchangerate.host"
    }

    async fn rates(&self, state: &AppState, base: &str, quotes: &[String]) -> AppResult<Vec<FxQuote>> {
        if quotes.is_empty() {
            return Ok(Vec::new());
        }
        let root = state
            .setting_opt("exchangerate_host_base_url")
            .await
            .unwrap_or_else(|| DEFAULT_BASE.to_string());
        let key = state.setting_opt("exchangerate_host_key").await;

        let symbols = quotes.join(",");
        let mut req = state
            .http
            .get(format!("{}/live", root.trim_end_matches('/')))
            .query(&[("source", base), ("currencies", symbols.as_str())]);
        if let Some(k) = &key {
            req = req.query(&[("access_key", k.as_str())]);
        }

        let res = req.send().await?;
        if !res.status().is_success() {
            return Err(AppError::provider(PROVIDER, format!("HTTP {}", res.status())));
        }

        let body: LiveResponse = res.json().await?;
        if body.success == Some(false) {
            return Err(AppError::provider(PROVIDER, "upstream reported failure (check access key)"));
        }

        let as_of = body.date.unwrap_or_else(|| chrono::Utc::now().date_naive());
        let base_uc = base.to_uppercase();

        // The `/live` shape keys quotes as e.g. `USDINR`; the older `/latest`
        // shape keys them by the quote currency alone. Accept both.
        let mut out = Vec::new();
        for (k, rate) in body.quotes {
            let quote = k.strip_prefix(&base_uc).unwrap_or(&k).to_uppercase();
            out.push(FxQuote { base: base_uc.clone(), quote, rate, as_of });
        }
        for (quote, rate) in body.rates {
            out.push(FxQuote { base: base_uc.clone(), quote: quote.to_uppercase(), rate, as_of });
        }
        Ok(out)
    }
}
