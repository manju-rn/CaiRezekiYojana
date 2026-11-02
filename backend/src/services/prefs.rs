//! Each user's own preferences, and the administrator's overrides of them.
//!
//! Base currency, cost-basis method, financial year, performance periods
//! and page visibility are a user's own business and live in
//! `user_setting`. An administrator can pin a page's visibility for a user
//! with `override_visibility_<page>`; while it is set it wins, and the user
//! cannot change that page. Everything app-wide - data feeds, provider
//! keys, sign-in options - stays in `app_setting`, for administrators.

use std::collections::BTreeMap;
use std::str::FromStr;

use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
};

use crate::entities::user_setting;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::types::CostBasisMethod;

use super::visibility::{self, MODES, PAGES};

/// The preferences a user may set for themselves.
pub const USER_KEYS: &[&str] = &[
    "base_currency",
    "cost_basis_method",
    "financial_year_start_month",
    "performance_periods",
    "plan_sessions_enabled",
    "plan_sessions_max",
    "plan_sessions_download",
    "plan_sessions_upload",
    "yatra_sessions_enabled",
    "yatra_sessions_max",
    "yatra_sessions_download",
    "yatra_sessions_upload",
    // checked against the administrator's largest size, in the settings API
    "text_size_steps",
];

pub const OVERRIDE_PREFIX: &str = "override_";

pub fn override_key(page: &str) -> String {
    format!("{OVERRIDE_PREFIX}{}", visibility::setting_key(page))
}

pub async fn all(db: &DatabaseConnection, user_id: i32) -> AppResult<BTreeMap<String, String>> {
    Ok(user_setting::Entity::find()
        .filter(user_setting::Column::UserId.eq(user_id))
        .all(db)
        .await?
        .into_iter()
        .map(|r| (r.key, r.value))
        .collect())
}

pub async fn get_opt(db: &DatabaseConnection, user_id: i32, key: &str) -> Option<String> {
    user_setting::Entity::find()
        .filter(user_setting::Column::UserId.eq(user_id))
        .filter(user_setting::Column::Key.eq(key))
        .one(db)
        .await
        .ok()
        .flatten()
        .map(|r| r.value)
        .filter(|v| !v.is_empty())
}

pub async fn get(db: &DatabaseConnection, user_id: i32, key: &str, default: &str) -> String {
    get_opt(db, user_id, key).await.unwrap_or_else(|| default.to_string())
}

pub async fn set(db: &DatabaseConnection, user_id: i32, key: &str, value: &str) -> AppResult<()> {
    let now = chrono::Utc::now().naive_utc();
    let existing = user_setting::Entity::find()
        .filter(user_setting::Column::UserId.eq(user_id))
        .filter(user_setting::Column::Key.eq(key))
        .one(db)
        .await?;
    match existing {
        Some(row) => {
            let mut m: user_setting::ActiveModel = row.into();
            m.value = Set(value.to_string());
            m.updated_at = Set(now);
            m.update(db).await?;
        }
        None => {
            user_setting::ActiveModel {
                user_id: Set(user_id),
                key: Set(key.to_string()),
                value: Set(value.to_string()),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(db)
            .await?;
        }
    }
    Ok(())
}

pub async fn remove(db: &DatabaseConnection, user_id: i32, key: &str) -> AppResult<()> {
    user_setting::Entity::delete_many()
        .filter(user_setting::Column::UserId.eq(user_id))
        .filter(user_setting::Column::Key.eq(key))
        .exec(db)
        .await?;
    Ok(())
}

/// The currency the user's totals are reported in: their own choice, else
/// the app-wide default for new users, else INR.
pub async fn base_currency(state: &AppState, user_id: i32) -> String {
    match get_opt(&state.db, user_id, "base_currency").await {
        Some(c) => c.to_uppercase(),
        None => state.setting("default_base_currency", "SGD").await.to_uppercase(),
    }
}

pub async fn cost_basis_method(state: &AppState, user_id: i32) -> CostBasisMethod {
    CostBasisMethod::from_str(&get(&state.db, user_id, "cost_basis_method", "FIFO").await)
        .unwrap_or(CostBasisMethod::Fifo)
}

/// A page's visibility as the user experiences it, and whether an
/// administrator has pinned it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct PageVisibility {
    pub page: &'static str,
    pub mode: String,
    /// The user's own choice, shown once an override is lifted.
    pub own: String,
    /// Set by an administrator; the user cannot change the page while it is.
    pub overridden: Option<String>,
}

pub fn effective(settings: &BTreeMap<String, String>) -> Vec<PageVisibility> {
    let valid = |v: Option<&String>| v.filter(|v| MODES.contains(&v.as_str())).cloned();
    PAGES
        .iter()
        .map(|page| {
            let own = valid(settings.get(&visibility::setting_key(page))).unwrap_or_else(|| "visible".to_string());
            let overridden = valid(settings.get(&override_key(page)));
            PageVisibility { page, mode: overridden.clone().unwrap_or_else(|| own.clone()), own, overridden }
        })
        .collect()
}

pub async fn visibility(db: &DatabaseConnection, user_id: i32) -> AppResult<Vec<PageVisibility>> {
    Ok(effective(&all(db, user_id).await?))
}

/// Check a preference a user is setting for themselves.
pub fn validate_own(key: &str, value: &str, pinned: &[PageVisibility]) -> AppResult<()> {
    if let Some(page) = visibility::page_of(key) {
        if !MODES.contains(&value) {
            return Err(AppError::bad(format!("{key} must be one of: {}", MODES.join(", "))));
        }
        if pinned.iter().any(|p| p.page == page && p.overridden.is_some()) {
            return Err(AppError::forbidden(format!(
                "your administrator has set the visibility of this page; it cannot be changed here ({key})"
            )));
        }
        return Ok(());
    }
    if !USER_KEYS.contains(&key) {
        return Err(AppError::bad(format!("{key} is not a setting you can change")));
    }
    match key {
        "base_currency" => {
            if value.len() != 3 || !value.chars().all(|c| c.is_ascii_alphabetic()) {
                return Err(AppError::bad("base_currency must be a 3-letter ISO code"));
            }
        }
        "cost_basis_method" => {
            CostBasisMethod::from_str(value).map_err(AppError::bad)?;
        }
        "financial_year_start_month" => {
            let month: u32 = value
                .parse()
                .map_err(|_| AppError::bad("financial_year_start_month must be a number"))?;
            if !(1..=12).contains(&month) {
                return Err(AppError::bad("financial_year_start_month must be 1-12"));
            }
        }
        "performance_periods" => {
            super::performance::parse_periods(value).map_err(AppError::bad)?;
        }
        k if super::plan_sessions::SWITCHES.contains(&k) => {
            if value != "true" && value != "false" {
                return Err(AppError::bad(format!("{key} must be true or false")));
            }
        }
        k if super::plan_sessions::MAXES.contains(&k) => {
            if super::plan_sessions::parse_max(value).is_none() {
                return Err(AppError::bad(format!(
                    "{key} must be a whole number from 1 to {}",
                    super::plan_sessions::MOST_ALLOWED
                )));
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn an_override_wins_over_the_users_choice() {
        let v = effective(&map(&[
            ("visibility_global", "hidden"),
            ("override_visibility_global", "excluded"),
            ("visibility_cash", "hidden"),
        ]));
        let global = v.iter().find(|p| p.page == "global").unwrap();
        assert_eq!(global.mode, "excluded");
        assert_eq!(global.own, "hidden");
        let cash = v.iter().find(|p| p.page == "cash").unwrap();
        assert_eq!((cash.mode.as_str(), cash.overridden.as_deref()), ("hidden", None));
        let funds = v.iter().find(|p| p.page == "india_mf").unwrap();
        assert_eq!(funds.mode, "visible", "unset pages show");
    }

    #[test]
    fn a_pinned_page_cannot_be_changed_by_its_user() {
        let pinned = effective(&map(&[("override_visibility_cash", "hidden")]));
        assert!(validate_own("visibility_cash", "visible", &pinned).is_err());
        assert!(validate_own("visibility_global", "hidden", &pinned).is_ok());
    }

    #[test]
    fn users_cannot_write_app_wide_or_override_keys() {
        assert!(validate_own("fx_provider", "xe", &[]).is_err());
        assert!(validate_own("override_visibility_cash", "visible", &[]).is_err());
        assert!(validate_own("base_currency", "AUD", &[]).is_ok());
        assert!(validate_own("base_currency", "AUDX", &[]).is_err());
    }
}
