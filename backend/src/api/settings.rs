//! Settings: the preferences of the one owner - base currency, session options,
//! page visibility and text size. App-wide keys (data feeds) are not writable here.

use std::collections::BTreeMap;

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::auth::Owner;
use crate::error::{AppError, AppResult};
use crate::services::prefs::{self, PageVisibility};
use crate::services::text_size;
use crate::services::visibility;
use crate::state::AppState;

use super::openapi::tags;

/// App-wide choices a user sees but cannot change, e.g. which FX feed is
/// behind their rates.
const SHOWN_TO_USERS: &[&str] = &["fx_provider", "quote_provider", "nav_provider", "fund_holdings_enabled", "history_backfill_enabled"];

// ---------------------------------------------------------------------------
// A user's own
// ---------------------------------------------------------------------------

#[derive(Serialize, utoipa::ToSchema)]
pub struct SettingsResponse {
    /// The user's preferences, with each page's visibility as it applies to
    /// them - an administrator's override included.
    pub settings: BTreeMap<String, String>,
    /// Each page's visibility, and whether it is pinned.
    pub visibility: Vec<PageVisibility>,
    /// App-wide choices, read-only here.
    pub app: BTreeMap<String, String>,
    /// Kept for older clients: users hold no secrets.
    pub secrets_set: Vec<String>,
    /// The text-size pills' steps (their own, if set and within the
    /// administrator's limit) and the slider's limits.
    pub text_size: text_size::TextSize,
}

#[utoipa::path(
    get,
    path = "/settings",
    operation_id = "get_settings",
    tag = tags::USER_SETTINGS,
    summary = "The caller's own preferences and page visibility",
    responses((status = 200, body = SettingsResponse))
)]
pub async fn list(State(state): State<AppState>, Owner(owner): Owner) -> AppResult<Json<SettingsResponse>> {
    Ok(Json(own(&state, owner).await?))
}

async fn own(state: &AppState, owner: i32) -> AppResult<SettingsResponse> {
    let stored = prefs::all(&state.db, owner).await?;
    let pages = prefs::effective(&stored);
    let mut settings: BTreeMap<String, String> = stored
        .into_iter()
        .filter(|(k, _)| prefs::USER_KEYS.contains(&k.as_str()))
        .collect();
    settings.insert("base_currency".into(), prefs::base_currency(state, owner).await);
    for p in &pages {
        settings.insert(visibility::setting_key(p.page), p.mode.clone());
    }
    let mut app = BTreeMap::new();
    for key in SHOWN_TO_USERS {
        if let Some(v) = state.setting_opt(key).await {
            app.insert(key.to_string(), v);
        }
    }
    let text_size = text_size::for_user(state, owner).await;
    Ok(SettingsResponse { settings, visibility: pages, app, secrets_set: Vec::new(), text_size })
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateSettings {
    /// key -> value. Only the keys sent are changed.
    pub settings: BTreeMap<String, String>,
}

#[utoipa::path(
    put,
    path = "/settings",
    operation_id = "update_settings",
    tag = tags::USER_SETTINGS,
    summary = "Change some of the caller's own preferences",
    description = "Keys: `base_currency`, `cost_basis_method`, `financial_year_start_month`, `performance_periods`, `plan_sessions_enabled`, `plan_sessions_max` (1-100), `plan_sessions_download`, `plan_sessions_upload` (`true` or `false`, on unless turned off), the same four `yatra_sessions_*` for CaiYatra, `visibility_<page>`, `text_size_steps` (such as `5,10,15`: up to 5 steps going up, none past the administrator's `text_size_max`; blank goes back to the app-wide steps). App-wide keys are refused.",
    request_body = UpdateSettings,
    responses(
        (status = 200, body = SettingsResponse),
        (status = 400, description = "Unknown key or invalid value", body = crate::error::ErrorBody),
        (status = 403, description = "A page the administrator has pinned", body = crate::error::ErrorBody),
    )
)]
pub async fn update(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Json(body): Json<UpdateSettings>,
) -> AppResult<Json<SettingsResponse>> {
    let pages = prefs::visibility(&state.db, owner).await?;
    // Clients send back the whole form; a pinned page coming back unchanged
    // is not an attempt to change it.
    let changes: Vec<(&String, &String)> = body
        .settings
        .iter()
        .filter(|(k, v)| match visibility::page_of(k) {
            Some(page) => pages.iter().any(|p| p.page == page && &p.mode != *v),
            None => true,
        })
        .collect();
    let text_max = text_size::app(&state).await.max;
    for (key, value) in &changes {
        if key.as_str() == text_size::STEPS_KEY {
            if !value.trim().is_empty() {
                text_size::parse_steps(value, text_max).map_err(|e| AppError::bad(format!("{key}: {e}")))?;
            }
            continue;
        }
        prefs::validate_own(key, value, &pages)?;
    }
    for (key, value) in changes {
        // Blank steps: back to the app-wide ones.
        if key.as_str() == text_size::STEPS_KEY && value.trim().is_empty() {
            prefs::remove(&state.db, owner, key).await?;
            continue;
        }
        let value = if key == "base_currency" { value.to_uppercase() } else { value.clone() };
        prefs::set(&state.db, owner, key, &value).await?;
    }
    Ok(Json(own(&state, owner).await?))
}

