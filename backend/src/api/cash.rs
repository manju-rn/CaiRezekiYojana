//! Bank and savings balances.

use axum::extract::{Path, Query, State};
use axum::Json;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect,
};
use serde::{Deserialize, Serialize};

use crate::domain::fx::FxEngine;
use crate::auth::Owner;
use crate::entities::cash_balance;
use crate::error::{AppError, AppResult};
use crate::services::cash as cash_service;
use crate::services::{prefs, users};
use crate::state::AppState;

use super::Deleted;
use super::openapi::tags;

#[derive(Serialize, utoipa::ToSchema)]
pub struct CashResponse {
    pub summary: cash_service::CashSummary,
    pub accounts: Vec<cash_service::CashAccountView>,
}

#[utoipa::path(
    get,
    path = "/cash",
    operation_id = "list_cash",
    tag = tags::USER_CASH,
    summary = "The caller's bank accounts with their latest balances, and a summary",
    responses((status = 200, body = CashResponse))
)]
pub async fn list(State(state): State<AppState>, Owner(owner): Owner) -> AppResult<Json<CashResponse>> {
    let base = prefs::base_currency(&state, owner).await;
    let fx = FxEngine::load(&state.db, &base, Some(owner)).await?;
    let today = chrono::Utc::now().date_naive();

    let accounts = cash_service::list(&state.db, owner, &fx, today).await?;
    let summary = cash_service::summarise(&accounts, &base, &fx);
    Ok(Json(CashResponse { summary, accounts }))
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct BalanceParams {
    pub account_id: Option<i32>,
    pub limit: Option<u64>,
}

/// Snapshot history, newest first - the series behind a balance chart.
#[utoipa::path(
    get,
    path = "/cash/balances",
    operation_id = "list_cash_balances",
    tag = tags::USER_CASH,
    summary = "Balance snapshots, newest first",
    params(BalanceParams),
    responses((status = 200, body = [cash_balance::Model]))
)]
pub async fn list_balances(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Query(params): Query<BalanceParams>,
) -> AppResult<Json<Vec<cash_balance::Model>>> {
    let mut q = cash_balance::Entity::find()
        .filter(cash_balance::Column::AccountId.is_in(users::account_ids(&state.db, owner).await?))
        .order_by_desc(cash_balance::Column::AsOf);
    if let Some(id) = params.account_id {
        q = q.filter(cash_balance::Column::AccountId.eq(id));
    }
    Ok(Json(
        q.limit(params.limit.unwrap_or(500).min(5000)).all(&state.db).await?,
    ))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct BalanceInput {
    pub account_id: i32,
    pub as_of: chrono::NaiveDate,
    pub balance: f64,
    pub currency_code: String,
    pub notes: Option<String>,
}

/// Record today's balance. Re-posting the same date corrects that snapshot
/// rather than adding a second one.
#[utoipa::path(
    post,
    path = "/cash/balances",
    operation_id = "record_cash_balance",
    tag = tags::USER_CASH,
    summary = "Record a balance snapshot",
    description = "Posting the same account and date again corrects that snapshot.",
    request_body = BalanceInput,
    responses((status = 200, body = cash_balance::Model), (status = 400, body = crate::error::ErrorBody), (status = 404, description = "Not one of the caller's accounts", body = crate::error::ErrorBody))
)]
pub async fn record_balance(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Json(body): Json<BalanceInput>,
) -> AppResult<Json<cash_balance::Model>> {
    users::owned_account(&state.db, owner, body.account_id).await?;
    if body.currency_code.len() != 3 {
        return Err(AppError::bad("currency_code must be a 3-letter ISO code"));
    }

    let existing = cash_balance::Entity::find()
        .filter(cash_balance::Column::AccountId.eq(body.account_id))
        .filter(cash_balance::Column::AsOf.eq(body.as_of))
        .one(&state.db)
        .await?;

    let saved = match existing {
        Some(row) => {
            let mut m: cash_balance::ActiveModel = row.into();
            m.balance = Set(body.balance);
            m.currency_code = Set(body.currency_code.to_uppercase());
            m.source = Set("manual".to_string());
            m.notes = Set(body.notes.clone());
            m.update(&state.db).await?
        }
        None => cash_balance::ActiveModel {
            account_id: Set(body.account_id),
            as_of: Set(body.as_of),
            balance: Set(body.balance),
            currency_code: Set(body.currency_code.to_uppercase()),
            source: Set("manual".to_string()),
            notes: Set(body.notes.clone()),
            created_at: Set(chrono::Utc::now().naive_utc()),
            ..Default::default()
        }
        .insert(&state.db)
        .await?,
    };

    Ok(Json(saved))
}

#[utoipa::path(
    delete,
    path = "/cash/balances/{id}",
    operation_id = "delete_cash_balance",
    tag = tags::USER_CASH,
    summary = "Delete a balance snapshot",
    params(("id" = i32, Path, description = "Snapshot id")),
    responses((status = 200, body = Deleted), (status = 404, body = crate::error::ErrorBody))
)]
pub async fn delete_balance(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<i32>,
) -> AppResult<Json<Deleted>> {
    let result = cash_balance::Entity::delete_many()
        .filter(cash_balance::Column::Id.eq(id))
        .filter(cash_balance::Column::AccountId.is_in(users::account_ids(&state.db, owner).await?))
        .exec(&state.db)
        .await?;
    if result.rows_affected == 0 {
        return Err(AppError::not_found("cash balance", id));
    }
    Ok(Json(Deleted { deleted: true, id }))
}
