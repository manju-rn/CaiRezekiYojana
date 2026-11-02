//! Retirement planning: how long the corpus lasts at different withdrawals.

use axum::body::Bytes;
use axum::extract::State;
use axum::Json;

use crate::auth::Owner;
use crate::error::{AppError, AppResult};
use crate::services::retirement::{self, RetirementInput, RetirementPlan};
use crate::state::AppState;

use super::openapi::tags;

/// The Retirement screen's simulation. Nothing is stored: it is arithmetic
/// on the portfolio and the assumptions given, so it is safe to call as
/// often as needed.
#[utoipa::path(
    post,
    path = "/retirement/simulate",
    operation_id = "simulate_retirement",
    tag = tags::USER_RETIREMENT,
    summary = "How long the corpus lasts at different withdrawals",
    description = "Runs each withdrawal for 50 years: withdrawals grow with inflation, what is left earns \
                   the return, and each currency's part of the corpus moves against the base at its \
                   assumed drift. A plan starting this year starts today, so its first row covers only \
                   what is left of the year (`assumptions.first_year_fraction`): that share of the \
                   withdrawal, of recurring extras and of the return, with one-off extras paid whole. \
                   Later rows are full calendar years. Every field of the body is optional - the corpus and its currency split \
                   default to today's net worth, inflation to the base currency's ten-year average, the \
                   withdrawals to 3, 4, 5 and 6% of the corpus - and `assumptions` in the answer says \
                   what was used. Each scenario also gives the same plan drawn monthly and, where part of \
                   the corpus is in another currency, the spread currency moves alone could give. Nothing \
                   is stored.",
    request_body(content = Option<RetirementInput>, content_type = "application/json"),
    responses((status = 200, body = RetirementPlan), (status = 400, body = crate::error::ErrorBody))
)]
pub async fn simulate(
    State(state): State<AppState>,
    Owner(owner): Owner,
    body: Bytes,
) -> AppResult<Json<RetirementPlan>> {
    // Read by hand rather than as `Option<Json<_>>`, which would quietly run
    // the defaults on a body that does not parse.
    let input = if body.iter().all(u8::is_ascii_whitespace) {
        RetirementInput::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| AppError::bad(format!("invalid body: {e}")))?
    };
    Ok(Json(retirement::simulate(&state, owner, input).await?))
}
