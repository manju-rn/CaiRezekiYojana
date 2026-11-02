//! Reference data and health.

use axum::extract::State;
use axum::Json;
use sea_orm::{EntityTrait, QueryOrder};
use serde::Serialize;

use crate::entities::{currency, exchange};
use crate::auth::Me;
use crate::error::AppResult;
use crate::state::AppState;
use crate::types::*;

use super::openapi::tags;

/// The release, from the repo's VERSION file - bumped by
/// `scripts/release.sh` alongside each `cairezeki.<version>` git tag.
pub const APP_VERSION: &str = include_str!("../../../VERSION").trim_ascii();

#[derive(Serialize, utoipa::ToSchema)]
pub struct Health {
    pub status: &'static str,
    pub version: &'static str,
    /// `sqlite` or `postgres`.
    pub database: &'static str,
}

#[utoipa::path(
    get,
    path = "/health",
    operation_id = "health",
    tag = tags::PUBLIC,
    summary = "Liveness, release and database kind",
    responses((status = 200, body = Health))
)]
pub async fn health(State(state): State<AppState>) -> Json<Health> {
    Json(Health {
        status: "ok",
        version: APP_VERSION,
        database: if state.config.is_sqlite() { "sqlite" } else { "postgres" },
    })
}

#[utoipa::path(
    get,
    path = "/meta/currencies",
    operation_id = "list_currencies",
    tag = tags::REFERENCE,
    summary = "Every known currency",
    responses((status = 200, body = [currency::Model]))
)]
pub async fn list_currencies(State(state): State<AppState>, _: Me) -> AppResult<Json<Vec<currency::Model>>> {
    Ok(Json(
        currency::Entity::find()
            .order_by_asc(currency::Column::Code)
            .all(&state.db)
            .await?,
    ))
}

#[utoipa::path(
    get,
    path = "/meta/exchanges",
    operation_id = "list_exchanges",
    tag = tags::REFERENCE,
    summary = "Every known stock exchange",
    responses((status = 200, body = [exchange::Model]))
)]
pub async fn list_exchanges(State(state): State<AppState>, _: Me) -> AppResult<Json<Vec<exchange::Model>>> {
    Ok(Json(
        exchange::Entity::find()
            .order_by_asc(exchange::Column::Code)
            .all(&state.db)
            .await?,
    ))
}

/// Every accepted enum value, so the UI never hard-codes a dropdown that can
/// drift from what the backend will accept.
#[derive(Serialize, utoipa::ToSchema)]
pub struct Enums {
    pub asset_kinds: Vec<&'static str>,
    pub account_kinds: Vec<&'static str>,
    pub transaction_kinds: Vec<&'static str>,
    pub deposit_kinds: Vec<&'static str>,
    pub compounding: Vec<&'static str>,
    pub payout_frequencies: Vec<&'static str>,
    pub deposit_statuses: Vec<&'static str>,
    pub margin_modes: Vec<&'static str>,
    pub import_kinds: Vec<&'static str>,
    pub cost_basis_methods: Vec<&'static str>,
    pub scopes: Vec<&'static str>,
}

#[utoipa::path(
    get,
    path = "/meta/enums",
    operation_id = "list_enums",
    tag = tags::REFERENCE,
    summary = "Every accepted enum value",
    responses((status = 200, body = Enums))
)]
pub async fn list_enums(_: Me) -> Json<Enums> {
    macro_rules! codes {
        ($t:ty) => {
            <$t>::ALL.iter().map(|v| v.as_str()).collect()
        };
    }
    Json(Enums {
        asset_kinds: codes!(AssetKind),
        account_kinds: codes!(AccountKind),
        transaction_kinds: codes!(TxnKind),
        deposit_kinds: codes!(DepositKind),
        compounding: codes!(Compounding),
        payout_frequencies: codes!(PayoutFrequency),
        deposit_statuses: codes!(DepositStatus),
        margin_modes: codes!(MarginMode),
        import_kinds: codes!(ImportKind),
        cost_basis_methods: codes!(CostBasisMethod),
        scopes: vec!["all", "india_equity", "india_mf", "global"],
    })
}
