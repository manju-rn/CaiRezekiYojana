//! Historical NAV backfill for Indian mutual funds, via mfapi.in.
//!
//! AMFI's own feed (`nav_amfi.rs`) only ever publishes today's snapshot, so
//! it can never answer "what was this fund worth a month ago." mfapi.in
//! mirrors AMFI's data but keeps each scheme's full daily history since
//! launch, which is what lets XIRR and period returns (1d/1w/1m/3m/...)
//! look further back than "since this app started refreshing."

use chrono::NaiveDate;
use serde::Deserialize;

use crate::error::AppResult;
use crate::state::AppState;

use super::Nav;

const PROVIDER: &str = "mfapi";

#[derive(Deserialize)]
struct MfApiResponse {
    #[serde(default)]
    meta: MfApiMeta,
    #[serde(default)]
    data: Vec<MfApiPoint>,
}

#[derive(Default, Deserialize)]
struct MfApiMeta {
    #[serde(default)]
    scheme_name: String,
    /// Same wording as the AMFI heading, e.g. `Equity Scheme - Large Cap Fund`.
    #[serde(default)]
    scheme_category: Option<String>,
}

#[derive(Deserialize)]
struct MfApiPoint {
    date: String,
    nav: String,
}

/// Full daily NAV history for one AMFI scheme code, oldest first.
///
/// A malformed row (mfapi occasionally has a blank `nav`) is skipped rather
/// than failing the whole fetch - one bad day should not cost every other
/// day's history.
pub async fn history(state: &AppState, scheme_code: &str) -> AppResult<Vec<Nav>> {
    let url = format!("https://api.mfapi.in/mf/{scheme_code}");
    let res = state.http.get(&url).send().await?;
    if !res.status().is_success() {
        return Err(crate::error::AppError::provider(PROVIDER, format!("HTTP {}", res.status())));
    }
    let body: MfApiResponse = res.json().await?;

    let mut out = Vec::with_capacity(body.data.len());
    for point in body.data {
        let Ok(nav) = point.nav.parse::<f64>() else { continue };
        let Some(as_of) = NaiveDate::parse_from_str(&point.date, "%d-%m-%Y").ok() else { continue };
        out.push(Nav {
            scheme_code: scheme_code.to_string(),
            scheme_name: body.meta.scheme_name.clone(),
            isin: None,
            nav,
            as_of,
            category: body.meta.scheme_category.clone().filter(|c| !c.trim().is_empty()),
        });
    }
    out.sort_by_key(|n| n.as_of);
    Ok(out)
}

/// The scheme's name as AMFI publishes it, from mfapi's `latest` endpoint
/// - one NAV rather than the whole series. A second spelling to search a
/// portfolio feed with when the statement's abbreviation finds nothing.
pub async fn scheme_name(state: &AppState, scheme_code: &str) -> AppResult<Option<String>> {
    let url = format!("https://api.mfapi.in/mf/{scheme_code}/latest");
    let res = state.http.get(&url).send().await?;
    if !res.status().is_success() {
        return Err(crate::error::AppError::provider(PROVIDER, format!("HTTP {}", res.status())));
    }
    let body: MfApiResponse = res.json().await?;
    Ok(Some(body.meta.scheme_name).filter(|n| !n.trim().is_empty()))
}
