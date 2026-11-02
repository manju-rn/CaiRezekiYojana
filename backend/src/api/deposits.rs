//! Fixed and recurring deposits across India, Singapore and Australia.

use axum::extract::{Path, Query, State};
use axum::Json;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::domain::deposits::value_on;
use crate::domain::fx::FxEngine;
use crate::auth::Owner;
use crate::entities::deposit;
use crate::error::{AppError, AppResult};
use crate::services::deposits as deposit_service;
use crate::services::{prefs, users};
use crate::state::AppState;
use crate::types::{Compounding, DepositKind, DepositStatus, PayoutFrequency};

use super::Deleted;
use super::openapi::tags;

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListParams {
    /// Valuation date. Defaults to today; set it to project a deposit forward.
    pub as_of: Option<chrono::NaiveDate>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DepositListResponse {
    pub summary: deposit_service::DepositSummary,
    pub deposits: Vec<deposit_service::DepositView>,
}

#[utoipa::path(
    get,
    path = "/deposits",
    operation_id = "list_deposits",
    tag = tags::USER_DEPOSITS,
    summary = "The caller's deposits, valued, with a summary",
    params(ListParams),
    responses((status = 200, body = DepositListResponse), (status = 400, body = crate::error::ErrorBody))
)]
pub async fn list(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Query(params): Query<ListParams>,
) -> AppResult<Json<DepositListResponse>> {
    let base = prefs::base_currency(&state, owner).await;
    let fx = FxEngine::load(&state.db, &base, Some(owner)).await?;
    let as_of = params.as_of.unwrap_or_else(|| chrono::Utc::now().date_naive());

    let deposits = deposit_service::list(&state.db, owner, &fx, as_of).await?;
    let summary = deposit_service::summarise(&deposits, &base, as_of, &fx);
    Ok(Json(DepositListResponse { summary, deposits }))
}

#[utoipa::path(
    get,
    path = "/deposits/summary",
    operation_id = "get_deposits_summary",
    tag = tags::USER_DEPOSITS,
    summary = "Totals across the caller's deposits",
    params(ListParams),
    responses((status = 200, body = deposit_service::DepositSummary))
)]
pub async fn summary(
    State(state): State<AppState>,
    owner: Owner,
    Query(params): Query<ListParams>,
) -> AppResult<Json<deposit_service::DepositSummary>> {
    let response = list(State(state), owner, Query(params)).await?;
    Ok(Json(response.0.summary))
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct DepositDetail {
    #[serde(flatten)]
    pub deposit: deposit::Model,
    pub valuation: crate::domain::deposits::DepositValuation,
}

#[utoipa::path(
    get,
    path = "/deposits/{id}",
    operation_id = "get_deposit",
    tag = tags::USER_DEPOSITS,
    summary = "One deposit",
    params(("id" = i32, Path, description = "Deposit id")),
    responses((status = 200, body = DepositDetail), (status = 404, body = crate::error::ErrorBody))
)]
pub async fn get_one(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<i32>,
) -> AppResult<Json<DepositDetail>> {
    let found = owned(&state, owner, id).await?;
    let valuation = value_on(&found, chrono::Utc::now().date_naive());
    Ok(Json(DepositDetail { deposit: found, valuation }))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct DepositInput {
    pub account_id: i32,
    pub kind: String,
    pub institution: String,
    pub reference: Option<String>,
    pub principal: f64,
    pub interest_rate: f64,
    pub compounding: String,
    pub payout_frequency: String,
    pub installment: Option<f64>,
    pub start_date: chrono::NaiveDate,
    pub maturity_date: chrono::NaiveDate,
    pub currency_code: String,
    pub country: Option<String>,
    pub auto_renew: Option<bool>,
    pub status: Option<String>,
    pub maturity_amount: Option<f64>,
    /// Tax withheld at source on the interest, as a percentage - `31.2` for
    /// an Indian NRO deposit. Omit or `null` when none is withheld.
    pub tds_pct: Option<f64>,
    pub notes: Option<String>,
}

impl DepositInput {
    fn validate(&self) -> AppResult<DepositKind> {
        let kind = DepositKind::from_str(&self.kind).map_err(AppError::bad)?;
        Compounding::from_str(&self.compounding).map_err(AppError::bad)?;
        PayoutFrequency::from_str(&self.payout_frequency).map_err(AppError::bad)?;
        if let Some(s) = &self.status {
            DepositStatus::from_str(s).map_err(AppError::bad)?;
        }
        if self.maturity_date <= self.start_date {
            return Err(AppError::bad("maturity_date must be after start_date"));
        }
        if !(0.0..=100.0).contains(&self.interest_rate) {
            return Err(AppError::bad("interest_rate must be between 0 and 100"));
        }
        if self.tds_pct.is_some_and(|t| !(0.0..=100.0).contains(&t)) {
            return Err(AppError::bad("tds_pct must be between 0 and 100"));
        }
        if kind == DepositKind::RecurringDeposit {
            if self.installment.unwrap_or(0.0) <= 0.0 {
                return Err(AppError::bad("a recurring deposit needs a positive installment"));
            }
        } else if self.principal <= 0.0 {
            return Err(AppError::bad("principal must be greater than zero"));
        }
        if self.currency_code.len() != 3 {
            return Err(AppError::bad("currency_code must be a 3-letter ISO code"));
        }
        Ok(kind)
    }

    fn country_code(&self) -> String {
        self.country.clone().map(|c| c.to_uppercase()).unwrap_or_else(|| {
            crate::importer::apply::country_for_currency(&self.currency_code).to_string()
        })
    }
}

#[utoipa::path(
    post,
    path = "/deposits",
    operation_id = "create_deposit",
    tag = tags::USER_DEPOSITS,
    summary = "Create a deposit",
    request_body = DepositInput,
    responses((status = 200, body = deposit::Model), (status = 400, body = crate::error::ErrorBody), (status = 404, description = "A referenced record is not the caller's", body = crate::error::ErrorBody), (status = 409, body = crate::error::ErrorBody))
)]
pub async fn create(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Json(body): Json<DepositInput>,
) -> AppResult<Json<deposit::Model>> {
    body.validate()?;
    users::owned_account(&state.db, owner, body.account_id).await?;
    let now = chrono::Utc::now().naive_utc();

    let saved = deposit::ActiveModel {
        account_id: Set(body.account_id),
        kind: Set(body.kind.to_uppercase()),
        institution: Set(body.institution.clone()),
        reference: Set(body.reference.clone()),
        principal: Set(body.principal),
        interest_rate: Set(body.interest_rate),
        compounding: Set(body.compounding.to_uppercase()),
        payout_frequency: Set(body.payout_frequency.to_uppercase()),
        installment: Set(body.installment),
        start_date: Set(body.start_date),
        maturity_date: Set(body.maturity_date),
        currency_code: Set(body.currency_code.to_uppercase()),
        country: Set(body.country_code()),
        auto_renew: Set(body.auto_renew.unwrap_or(false)),
        status: Set(body
            .status
            .clone()
            .unwrap_or_else(|| DepositStatus::Active.to_string())
            .to_uppercase()),
        maturity_amount: Set(body.maturity_amount),
        tds_pct: Set(body.tds_pct),
        notes: Set(body.notes.clone()),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(&state.db)
    .await?;

    Ok(Json(saved))
}

#[utoipa::path(
    put,
    path = "/deposits/{id}",
    operation_id = "update_deposit",
    tag = tags::USER_DEPOSITS,
    summary = "Replace a deposit",
    params(("id" = i32, Path, description = "Deposit id")),
    request_body = DepositInput,
    responses((status = 200, body = deposit::Model), (status = 400, body = crate::error::ErrorBody), (status = 404, body = crate::error::ErrorBody), (status = 409, body = crate::error::ErrorBody))
)]
pub async fn update(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<i32>,
    Json(body): Json<DepositInput>,
) -> AppResult<Json<deposit::Model>> {
    body.validate()?;
    let existing = owned(&state, owner, id).await?;
    users::owned_account(&state.db, owner, body.account_id).await?;

    let mut m: deposit::ActiveModel = existing.into();
    m.account_id = Set(body.account_id);
    m.kind = Set(body.kind.to_uppercase());
    m.institution = Set(body.institution.clone());
    m.reference = Set(body.reference.clone());
    m.principal = Set(body.principal);
    m.interest_rate = Set(body.interest_rate);
    m.compounding = Set(body.compounding.to_uppercase());
    m.payout_frequency = Set(body.payout_frequency.to_uppercase());
    m.installment = Set(body.installment);
    m.start_date = Set(body.start_date);
    m.maturity_date = Set(body.maturity_date);
    m.currency_code = Set(body.currency_code.to_uppercase());
    m.country = Set(body.country_code());
    m.auto_renew = Set(body.auto_renew.unwrap_or(false));
    m.status = Set(body
        .status
        .clone()
        .unwrap_or_else(|| DepositStatus::Active.to_string())
        .to_uppercase());
    m.maturity_amount = Set(body.maturity_amount);
    m.tds_pct = Set(body.tds_pct);
    m.notes = Set(body.notes.clone());
    m.updated_at = Set(chrono::Utc::now().naive_utc());

    Ok(Json(m.update(&state.db).await?))
}

#[utoipa::path(
    delete,
    path = "/deposits/{id}",
    operation_id = "delete_deposit",
    tag = tags::USER_DEPOSITS,
    summary = "Delete a deposit",
    params(("id" = i32, Path, description = "Deposit id")),
    responses((status = 200, body = Deleted), (status = 404, body = crate::error::ErrorBody), (status = 409, body = crate::error::ErrorBody))
)]
pub async fn remove(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<i32>,
) -> AppResult<Json<Deleted>> {
    owned(&state, owner, id).await?;
    deposit::Entity::delete_by_id(id).exec(&state.db).await?;
    Ok(Json(Deleted { deleted: true, id }))
}

/// A deposit in one of `owner`'s accounts; anyone else's is not found.
async fn owned(state: &AppState, owner: i32, id: i32) -> AppResult<deposit::Model> {
    deposit::Entity::find_by_id(id)
        .filter(deposit::Column::AccountId.is_in(users::account_ids(&state.db, owner).await?))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("deposit", id))
}
