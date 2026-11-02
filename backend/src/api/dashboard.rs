//! The single-screen view of everything.

use axum::extract::State;
use axum::Json;

use crate::auth::{Owner, Refresher};
use crate::error::AppResult;
use crate::services::networth::{self, NetWorth};
use crate::services::refresh::{self, RefreshReport};
use crate::state::AppState;

use super::openapi::tags;

#[utoipa::path(
    get,
    path = "/dashboard",
    operation_id = "get_net_worth",
    tag = tags::USER_DASHBOARD,
    summary = "Net worth in the caller's base currency, with what it had to assume",
    responses((status = 200, body = NetWorth))
)]
pub async fn net_worth(State(state): State<AppState>, Owner(owner): Owner) -> AppResult<Json<NetWorth>> {
    Ok(Json(networth::build(&state, owner).await?))
}

/// Pull FX, then quotes, then NAVs. FX runs first so freshly fetched prices
/// are immediately convertible.
#[utoipa::path(
    post,
    path = "/dashboard/refresh",
    operation_id = "refresh_all",
    tag = tags::REFRESH,
    summary = "Refresh FX rates, then quotes, then NAVs",
    description = "What the nightly job calls. Feed failures come back as warnings in the reports.",
    responses((status = 200, body = [RefreshReport]))
)]
pub async fn refresh_all(State(state): State<AppState>, _: Refresher) -> AppResult<Json<Vec<RefreshReport>>> {
    Ok(Json(refresh::refresh_all(&state).await?))
}
