//! Derived positions, per scope.

use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use std::str::FromStr;

use crate::domain::fx::FxEngine;
use crate::error::{AppError, AppResult};
use crate::auth::Owner;
use crate::services::prefs;
use crate::services::portfolio::{self, PortfolioView, Scope};
use crate::state::AppState;

use super::openapi::tags;

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct HoldingsParams {
    /// `all`, `india_equity`, `india_mf` or `global`. Defaults to `all`: every
    /// listed holding - Indian stocks, Indian funds and global. Deposits and
    /// cash are not holdings: see `/deposits` and `/cash`, or `/dashboard` for
    /// net worth.
    pub scope: Option<String>,
    pub account_id: Option<i32>,
    /// Include positions that have been fully sold - needed for realised P&L.
    pub include_closed: Option<bool>,
    /// Report in this currency instead of the configured base.
    pub base: Option<String>,
}

#[utoipa::path(
    get,
    path = "/holdings",
    operation_id = "list_holdings",
    tag = tags::USER_HOLDINGS,
    summary = "The caller's positions, valued, for one scope",
    description = "Each position carries its native-currency figures, the base-currency figures and the rate that converted them.",
    params(HoldingsParams),
    responses((status = 200, body = PortfolioView), (status = 400, body = crate::error::ErrorBody))
)]
pub async fn list(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Query(params): Query<HoldingsParams>,
) -> AppResult<Json<PortfolioView>> {
    let scope = Scope::from_str(params.scope.as_deref().unwrap_or("all")).map_err(AppError::bad)?;

    let base = match params.base {
        Some(b) if b.len() == 3 => b.to_uppercase(),
        Some(_) => return Err(AppError::bad("base must be a 3-letter ISO code")),
        None => prefs::base_currency(&state, owner).await,
    };

    let fx = FxEngine::load(&state.db, &base, Some(owner)).await?;
    let method = prefs::cost_basis_method(&state, owner).await;

    Ok(Json(
        portfolio::build(
            &state.db,
            &fx,
            Some(owner),
            scope,
            method,
            params.account_id,
            params.include_closed.unwrap_or(false),
        )
        .await?,
    ))
}
