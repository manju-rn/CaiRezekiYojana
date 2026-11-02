//! FX rates from Xe.com's commercial Currency Data API.
//!
//! A note on how this is wired, because it matters: xe.com's website has no
//! free public API, and scraping the page would be both brittle and against
//! their terms. This adapter therefore targets the paid XE Currency Data API
//! (`xecdapi.xe.com`), authenticated with an account ID and key that you
//! supply in Settings.
//!
//! Until those credentials are present the provider reports itself as
//! unconfigured and the app stays on the free default feed, so nothing
//! breaks if you never sign up.

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

use super::{FxProvider, FxQuote};

const DEFAULT_BASE: &str = "https://xecdapi.xe.com/v1";
const PROVIDER: &str = "xe";

pub struct XeProvider;

#[derive(Deserialize)]
struct ConvertFromResponse {
    #[serde(default)]
    from: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    to: Vec<XeQuoteRow>,
}

#[derive(Deserialize)]
struct XeQuoteRow {
    quotecurrency: String,
    /// XE returns `mid`, plus `bid`/`ask` on higher tiers. Mid is the right
    /// input here - a dealing spread belongs in the margin setting, where it
    /// is visible and editable, not baked into the stored rate.
    mid: f64,
}

#[async_trait]
impl FxProvider for XeProvider {
    fn id(&self) -> &'static str {
        PROVIDER
    }

    fn label(&self) -> &'static str {
        "Xe.com Currency Data API"
    }

    async fn is_configured(&self, state: &AppState) -> bool {
        state.setting_opt("xe_api_id").await.is_some() && state.setting_opt("xe_api_key").await.is_some()
    }

    async fn rates(&self, state: &AppState, base: &str, quotes: &[String]) -> AppResult<Vec<FxQuote>> {
        if quotes.is_empty() {
            return Ok(Vec::new());
        }

        let (Some(api_id), Some(api_key)) =
            (state.setting_opt("xe_api_id").await, state.setting_opt("xe_api_key").await)
        else {
            return Err(AppError::provider(
                PROVIDER,
                "missing credentials - set xe_api_id and xe_api_key in Settings, or switch the FX provider",
            ));
        };

        let root = state
            .setting_opt("xe_base_url")
            .await
            .unwrap_or_else(|| DEFAULT_BASE.to_string());

        let symbols = quotes.join(",");
        let res = state
            .http
            .get(format!("{}/convert_from", root.trim_end_matches('/')))
            .basic_auth(api_id, Some(api_key))
            .query(&[("from", base), ("to", symbols.as_str()), ("amount", "1")])
            .send()
            .await?;

        let status = res.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(AppError::provider(PROVIDER, "credentials rejected by xe.com"));
        }
        if !status.is_success() {
            return Err(AppError::provider(PROVIDER, format!("HTTP {status}")));
        }

        let body: ConvertFromResponse = res.json().await?;
        let as_of = parse_xe_timestamp(body.timestamp.as_deref());
        let base_uc = body.from.unwrap_or_else(|| base.to_string()).to_uppercase();

        Ok(body
            .to
            .into_iter()
            .map(|row| FxQuote {
                base: base_uc.clone(),
                quote: row.quotecurrency.to_uppercase(),
                rate: row.mid,
                as_of,
            })
            .collect())
    }
}

/// XE stamps responses with an RFC-3339 instant; we only keep the date.
fn parse_xe_timestamp(raw: Option<&str>) -> NaiveDate {
    raw.and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map(|dt| dt.with_timezone(&Utc).date_naive())
        .unwrap_or_else(|| Utc::now().date_naive())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_date_out_of_an_xe_timestamp() {
        assert_eq!(
            parse_xe_timestamp(Some("2026-09-18T14:05:00Z")),
            NaiveDate::from_ymd_opt(2026, 9, 18).unwrap()
        );
    }

    #[test]
    fn falls_back_to_today_when_the_stamp_is_missing_or_junk() {
        let today = Utc::now().date_naive();
        assert_eq!(parse_xe_timestamp(None), today);
        assert_eq!(parse_xe_timestamp(Some("not-a-date")), today);
    }
}
