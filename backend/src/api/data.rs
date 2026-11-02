//! Deleting the caller's records in bulk - the danger zone in Settings.

use axum::extract::State;
use axum::Json;
use serde::Deserialize;

use crate::auth::Owner;
use crate::error::AppResult;
use crate::services::purge::{self, Category, Purged};
use crate::state::AppState;

use super::openapi::tags;

#[utoipa::path(
    get,
    path = "/data",
    operation_id = "list_deletable_records",
    tag = tags::USER_DATA,
    summary = "What the caller could delete, by page and institution",
    responses((status = 200, description = "Every page, in menu order", body = [Category]))
)]
pub async fn inventory(State(state): State<AppState>, Owner(owner): Owner) -> AppResult<Json<Vec<Category>>> {
    Ok(Json(purge::inventory(&state.db, owner).await?))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct DeleteInput {
    /// `india_equity`, `india_mf`, `global`, `deposits` or `cash`; omit or
    /// `null` for every page.
    pub category: Option<String>,
    /// A name from `GET /data`; omit or `null` for every institution.
    pub institution: Option<String>,
    /// Must be `DELETE`, exactly.
    pub confirm: String,
}

/// Delete records for good. Accounts left empty go with them; with neither
/// a category nor an institution, every account and the import history go.
#[utoipa::path(
    post,
    path = "/data/delete",
    operation_id = "delete_my_records",
    tag = tags::USER_DATA,
    summary = "Delete records by page and institution - cannot be undone",
    description = "Deleted records cannot be restored. `confirm` must be `DELETE`. The account itself, its settings and the upload audit are kept.",
    request_body = DeleteInput,
    responses((status = 200, description = "What was deleted", body = Purged), (status = 400, description = "Not confirmed, or an unknown category", body = crate::error::ErrorBody))
)]
pub async fn delete(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Json(body): Json<DeleteInput>,
) -> AppResult<Json<Purged>> {
    purge::check_confirmation(&body.confirm)?;
    Ok(Json(purge::delete(&state.db, owner, chosen(&body.category), chosen(&body.institution)).await?))
}

/// A picker's value, with blank meaning "all".
fn chosen(v: &Option<String>) -> Option<&str> {
    v.as_deref().map(str::trim).filter(|s| !s.is_empty())
}
