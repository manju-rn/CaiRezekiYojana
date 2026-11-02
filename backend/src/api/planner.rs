//! RezekiYojana, the allocation planner: the plans themselves.
//!
//! The web app reads and writes these too, so a change made here - by a
//! script or an assistant over MCP - shows on the page within seconds. See
//! `services::planner` for what a plan is.

use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::auth::Owner;
use crate::entities::plan_session;
use crate::error::{AppError, AppResult};
use crate::services::plan_sessions::{self, Kind};
use crate::services::planner::{self, NewItem, NewMove, NewSubInput, Plan, PlanState, View};
use crate::state::AppState;

use super::openapi::tags;

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PlanParams {
    /// Work the plan out against today's values: each category today, moved in and out, planned,
    /// and what can still move out of it. Values the portfolio, so slower.
    pub summary: Option<bool>,
}

#[utoipa::path(
    get,
    path = "/planner/{view}",
    operation_id = "get_plan",
    tag = tags::USER_PLANNER,
    summary = "The caller's allocation plan for a view, and its revision",
    description = "A plan moves money out of what is held today - a category, one fund in it, or fresh money - into \
                   a category held today or one the plan makes. Nothing in it is a transaction: it is always worked \
                   out against today's values. With `summary=true` the answer has each category's today, planned \
                   and available values and the keys to use in the other planner calls. The web app shows the same \
                   plan, and picks up a change made here within seconds.",
    params(("view" = View, Path, description = "`segments` (funds, ETFs, stocks, deposits, cash), the one plan. `classes` (the equity-debt line) reads the same plan and cannot change it"), PlanParams),
    responses((status = 200, body = PlanState))
)]
pub async fn get(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(view): Path<View>,
    Query(q): Query<PlanParams>,
) -> AppResult<Json<PlanState>> {
    Ok(Json(planner::get(&state, owner, view, q.summary.unwrap_or(false)).await?))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ReplacePlan {
    pub plan: Plan,
    /// The revision the plan was read at. When given and the plan has moved on since, nothing is
    /// written and the answer is `409`: read it again and reapply the change.
    #[serde(default)]
    pub revision: Option<i64>,
}

#[utoipa::path(
    put,
    path = "/planner/{view}",
    operation_id = "replace_plan",
    tag = tags::USER_PLANNER,
    summary = "Replace a whole plan",
    description = "What the web app saves with. Prefer the finer calls - add_plan_move, add_plan_category and the \
                   rest - which check the plan against today's holdings; this checks only that the plan is \
                   consistent in itself.",
    params(("view" = View, Path, description = "`segments` (funds, ETFs, stocks, deposits, cash), the one plan. `classes` (the equity-debt line) reads the same plan and cannot change it")),
    request_body = ReplacePlan,
    responses(
        (status = 200, body = PlanState),
        (status = 400, body = crate::error::ErrorBody),
        (status = 409, description = "The plan has changed since `revision`", body = crate::error::ErrorBody)
    )
)]
pub async fn replace(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(view): Path<View>,
    Json(body): Json<ReplacePlan>,
) -> AppResult<Json<PlanState>> {
    Ok(Json(planner::replace(&state, owner, view, body.plan, body.revision).await?))
}

#[utoipa::path(
    delete,
    path = "/planner/{view}",
    operation_id = "reset_plan",
    tag = tags::USER_PLANNER,
    summary = "Clear a plan: every move, new category and fund, and the fresh money",
    params(("view" = View, Path, description = "`segments` (funds, ETFs, stocks, deposits, cash), the one plan. `classes` (the equity-debt line) reads the same plan and cannot change it")),
    responses((status = 200, body = PlanState))
)]
pub async fn reset(State(state): State<AppState>, Owner(owner): Owner, Path(view): Path<View>) -> AppResult<Json<PlanState>> {
    Ok(Json(planner::reset(&state, owner, view).await?))
}

#[utoipa::path(
    post,
    path = "/planner/{view}/moves",
    operation_id = "add_plan_move",
    tag = tags::USER_PLANNER,
    summary = "Plan a move: money out of a category held today (or fresh money) into another",
    description = "By `amount` in the base currency, or by `pct` of what the source holds today. A source cannot \
                   give more than it has left after the moves already planned out of it. `made` in the answer is \
                   the new move's id.",
    params(("view" = View, Path, description = "`segments` (funds, ETFs, stocks, deposits, cash), the one plan. `classes` (the equity-debt line) reads the same plan and cannot change it")),
    request_body = NewMove,
    responses((status = 200, body = PlanState), (status = 400, body = crate::error::ErrorBody))
)]
pub async fn add_move(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(view): Path<View>,
    Json(body): Json<NewMove>,
) -> AppResult<Json<PlanState>> {
    Ok(Json(planner::add_move(&state, owner, view, body).await?))
}

#[utoipa::path(
    delete,
    path = "/planner/{view}/moves/{id}",
    operation_id = "remove_plan_move",
    tag = tags::USER_PLANNER,
    summary = "Undo a planned move",
    params(("view" = View, Path, description = "`segments` (funds, ETFs, stocks, deposits, cash), the one plan. `classes` (the equity-debt line) reads the same plan and cannot change it"), ("id" = String, Path, description = "The move's id")),
    responses((status = 200, body = PlanState), (status = 404, body = crate::error::ErrorBody))
)]
pub async fn remove_move(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path((view, id)): Path<(View, String)>,
) -> AppResult<Json<PlanState>> {
    Ok(Json(planner::remove_move(&state, owner, view, &id).await?))
}

#[utoipa::path(
    post,
    path = "/planner/{view}/categories",
    operation_id = "add_plan_category",
    tag = tags::USER_PLANNER,
    summary = "Make a new category in the plan, with its classification",
    description = "`made` in the answer is its key, the `to` of a move into it.",
    params(("view" = View, Path, description = "`segments` (funds, ETFs, stocks, deposits, cash), the one plan. `classes` (the equity-debt line) reads the same plan and cannot change it")),
    request_body = NewItem,
    responses((status = 200, body = PlanState), (status = 400, body = crate::error::ErrorBody))
)]
pub async fn add_category(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(view): Path<View>,
    Json(body): Json<NewItem>,
) -> AppResult<Json<PlanState>> {
    Ok(Json(planner::add_category(&state, owner, view, body).await?))
}

#[utoipa::path(
    delete,
    path = "/planner/{view}/categories/{key}",
    operation_id = "remove_plan_category",
    tag = tags::USER_PLANNER,
    summary = "Delete a category from the plan",
    description = "A category the plan made goes with every move into or out of it, and its funds. One held today \
                   has the moves into it undone and is set aside - only once everything in it is planned out \
                   elsewhere (`409` until then); restore_plan_category brings it back.",
    params(("view" = View, Path, description = "`segments` (funds, ETFs, stocks, deposits, cash), the one plan. `classes` (the equity-debt line) reads the same plan and cannot change it"), ("key" = String, Path, description = "The category's key")),
    responses(
        (status = 200, body = PlanState),
        (status = 404, body = crate::error::ErrorBody),
        (status = 409, description = "It still holds money not planned elsewhere", body = crate::error::ErrorBody)
    )
)]
pub async fn remove_category(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path((view, key)): Path<(View, String)>,
) -> AppResult<Json<PlanState>> {
    Ok(Json(planner::remove_category(&state, owner, view, &key).await?))
}

#[utoipa::path(
    post,
    path = "/planner/{view}/categories/{key}/restore",
    operation_id = "restore_plan_category",
    tag = tags::USER_PLANNER,
    summary = "Bring back one of today's categories deleted from the plan",
    params(("view" = View, Path, description = "`segments` (funds, ETFs, stocks, deposits, cash), the one plan. `classes` (the equity-debt line) reads the same plan and cannot change it"), ("key" = String, Path, description = "The category's key")),
    responses((status = 200, body = PlanState), (status = 404, body = crate::error::ErrorBody))
)]
pub async fn restore_category(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path((view, key)): Path<(View, String)>,
) -> AppResult<Json<PlanState>> {
    Ok(Json(planner::restore_category(&state, owner, view, &key).await?))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SetClass {
    /// One of `equity_fund`, `equity_stock`, `debt_high`, `debt_low`, `gold`, `real_estate`, `other`;
    /// `null` for one of today's categories goes back to what it holds.
    pub cls: Option<String>,
}

#[utoipa::path(
    put,
    path = "/planner/{view}/categories/{key}/class",
    operation_id = "set_plan_category_class",
    tag = tags::USER_PLANNER,
    summary = "Classify a category in the plan",
    params(("view" = View, Path, description = "`segments` (funds, ETFs, stocks, deposits, cash), the one plan. `classes` (the equity-debt line) reads the same plan and cannot change it"), ("key" = String, Path, description = "The category's key")),
    request_body = SetClass,
    responses((status = 200, body = PlanState), (status = 400, body = crate::error::ErrorBody), (status = 404, body = crate::error::ErrorBody))
)]
pub async fn set_class(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path((view, key)): Path<(View, String)>,
    Json(body): Json<SetClass>,
) -> AppResult<Json<PlanState>> {
    Ok(Json(planner::set_class(&state, owner, view, &key, body.cls).await?))
}

#[utoipa::path(
    post,
    path = "/planner/{view}/subcategories",
    operation_id = "add_plan_subcategory",
    tag = tags::USER_PLANNER,
    summary = "Make a new fund inside a category of the plan",
    description = "`made` in the answer is its key, the `to_sub` of a move into it.",
    params(("view" = View, Path, description = "`segments` (funds, ETFs, stocks, deposits, cash), the one plan. `classes` (the equity-debt line) reads the same plan and cannot change it")),
    request_body = NewSubInput,
    responses((status = 200, body = PlanState), (status = 400, body = crate::error::ErrorBody))
)]
pub async fn add_sub(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(view): Path<View>,
    Json(body): Json<NewSubInput>,
) -> AppResult<Json<PlanState>> {
    Ok(Json(planner::add_sub(&state, owner, view, body).await?))
}

#[utoipa::path(
    delete,
    path = "/planner/{view}/subcategories/{key}",
    operation_id = "remove_plan_subcategory",
    tag = tags::USER_PLANNER,
    summary = "Delete a fund the plan made, undoing the moves into it",
    params(("view" = View, Path, description = "`segments` (funds, ETFs, stocks, deposits, cash), the one plan. `classes` (the equity-debt line) reads the same plan and cannot change it"), ("key" = String, Path, description = "The fund's key (`sub-...`)")),
    responses((status = 200, body = PlanState), (status = 404, body = crate::error::ErrorBody))
)]
pub async fn remove_sub(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path((view, key)): Path<(View, String)>,
) -> AppResult<Json<PlanState>> {
    Ok(Json(planner::remove_sub(&state, owner, view, &key).await?))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct SetFresh {
    /// Money to invest on top of today's portfolio, in the base currency.
    pub amount: f64,
}

#[utoipa::path(
    put,
    path = "/planner/{view}/fresh",
    operation_id = "set_plan_fresh",
    tag = tags::USER_PLANNER,
    summary = "Set the fresh money the plan can move, on top of today's portfolio",
    params(("view" = View, Path, description = "`segments` (funds, ETFs, stocks, deposits, cash), the one plan. `classes` (the equity-debt line) reads the same plan and cannot change it")),
    request_body = SetFresh,
    responses((status = 200, body = PlanState), (status = 400, body = crate::error::ErrorBody))
)]
pub async fn set_fresh(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(view): Path<View>,
    Json(body): Json<SetFresh>,
) -> AppResult<Json<PlanState>> {
    Ok(Json(planner::set_fresh(&state, owner, view, body.amount).await?))
}


// ---------------------------------------------------------------------------
// Recorded sessions
// ---------------------------------------------------------------------------

/// A recorded session, without its contents.
#[derive(Serialize, utoipa::ToSchema)]
pub struct SessionListed {
    pub id: i32,
    pub name: String,
    /// When it was first saved, UTC.
    pub created_at: DateTime<Utc>,
    /// When it was uploaded from a downloaded file, UTC; absent: saved here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uploaded_at: Option<DateTime<Utc>>,
}

/// A page's recorded sessions, the planner's or CaiYatra's, and what the caller's settings for them
/// allow: `plan_sessions_*` for the planner, `yatra_sessions_*` for CaiYatra.
#[derive(Serialize, utoipa::ToSchema)]
pub struct SessionList {
    /// Newest first, by when each was first saved.
    pub sessions: Vec<SessionListed>,
    /// Whether sessions may be recorded: `<prefix>_enabled`, on unless turned off.
    pub enabled: bool,
    /// How many the caller may keep: `<prefix>_max`, 10 when not set.
    pub max: u32,
    /// Whether sessions may be downloaded: `<prefix>_download`, on unless turned off.
    pub download: bool,
    /// Whether downloaded sessions may be uploaded: `<prefix>_upload`, on unless turned off.
    pub upload: bool,
}

/// A recorded session in full.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PlanSession {
    pub id: i32,
    pub name: String,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uploaded_at: Option<DateTime<Utc>>,
    /// The plan as it stood when saved.
    pub plan: Plan,
    /// The values the planner page showed the plan against, in the page's own shape.
    #[schema(value_type = Object)]
    pub page: serde_json::Value,
}

impl TryFrom<plan_session::Model> for PlanSession {
    type Error = AppError;
    fn try_from(m: plan_session::Model) -> Result<Self, AppError> {
        let plan = plan_sessions::plan_of(&m)?.unwrap_or_default();
        Ok(PlanSession {
            id: m.id,
            name: m.name,
            created_at: m.created_at.and_utc(),
            uploaded_at: m.uploaded_at.map(|t| t.and_utc()),
            plan,
            page: serde_json::from_str(&m.page).map_err(|e| AppError::Other(e.into()))?,
        })
    }
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct NewPlanSession {
    /// 1 to 120 characters, unique among the caller's sessions (ignoring case).
    #[schema(example = "Before the March rebalance")]
    pub name: String,
    pub plan: Plan,
    /// The values the planner page shows the plan against, so the session reads later as it did
    /// now: `{version: 1, data, segSubs, clsSubs}`. Only the web app can fill it in.
    #[schema(value_type = Object)]
    pub page: serde_json::Value,
}

/// A planner session as export_plan_session gives it, and as import_plan_session takes it back.
#[derive(Deserialize, utoipa::ToSchema)]
#[allow(dead_code)] // read as a `plan_sessions::SessionDoc`; this is its planner shape, for the docs
pub struct PlanSessionDoc {
    /// `cairezeki-plan-session`.
    format: String,
    /// 1.
    version: u32,
    name: String,
    /// When the session was first saved, with its offset.
    created_at: DateTime<Utc>,
    /// When it was downloaded; ignored on upload.
    exported_at: Option<DateTime<Utc>>,
    /// The plan as it stood when saved.
    plan: Plan,
    /// The values the planner page showed the plan against: `{version: 1, data, segSubs, clsSubs}`.
    #[schema(value_type = Object)]
    page: serde_json::Value,
}

/// The planner page's (or its viewer's) own JSON download, as downloaded: what the page showed,
/// and the session under `session`. Its other fields are not read.
#[derive(Deserialize, utoipa::ToSchema)]
#[allow(dead_code)]
pub struct PlanPageDownload {
    /// `cairezeki-plan`.
    format: String,
    session: PlanSessionDoc,
}

/// A downloaded planner session, either way it is downloaded.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(untagged)]
#[allow(dead_code)]
pub enum PlanSessionUpload {
    Session(PlanSessionDoc),
    Page(PlanPageDownload),
}

pub(super) async fn session_list(state: &AppState, owner: i32, kind: Kind) -> AppResult<SessionList> {
    let opts = plan_sessions::options(&state.db, owner, kind).await;
    Ok(SessionList {
        sessions: plan_sessions::list(&state.db, owner, kind)
            .await?
            .into_iter()
            .map(|s| SessionListed {
                id: s.id,
                name: s.name,
                created_at: s.created_at.and_utc(),
                uploaded_at: s.uploaded_at.map(|t| t.and_utc()),
            })
            .collect(),
        enabled: opts.enabled,
        max: opts.max,
        download: opts.download,
        upload: opts.upload,
    })
}

#[utoipa::path(
    get,
    path = "/planner/recorded",
    operation_id = "list_plan_sessions",
    tag = tags::USER_PLANNER,
    summary = "The caller's recorded planner sessions, newest first, and what their settings allow",
    description = "A recorded session is the plan as it stood at a moment, kept with the values it was worked \
                   out against, under a name. It never changes once saved. `enabled`, `max`, `download` and \
                   `upload` are the caller's `plan_sessions_*` settings.",
    responses((status = 200, body = SessionList))
)]
pub async fn list_sessions(State(state): State<AppState>, Owner(owner): Owner) -> AppResult<Json<SessionList>> {
    Ok(Json(session_list(&state, owner, Kind::Planner).await?))
}

#[utoipa::path(
    post,
    path = "/planner/recorded",
    operation_id = "save_plan_session",
    tag = tags::USER_PLANNER,
    summary = "Record the plan as it stands, with the values the page shows it against",
    description = "What the planner page's Save session does. `403` when recording is turned off \
                   (`plan_sessions_enabled`); `409` when the name is taken or the caller keeps as many sessions \
                   as `plan_sessions_max` allows.",
    request_body = NewPlanSession,
    responses(
        (status = 200, body = SessionList),
        (status = 400, body = crate::error::ErrorBody),
        (status = 403, description = "Recording is turned off", body = crate::error::ErrorBody),
        (status = 409, description = "The name is taken, or as many sessions are kept as allowed", body = crate::error::ErrorBody)
    )
)]
pub async fn save_session(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Json(body): Json<NewPlanSession>,
) -> AppResult<Json<SessionList>> {
    plan_sessions::create(&state.db, owner, Kind::Planner, &body.name, Some(&body.plan), &body.page).await?;
    Ok(Json(session_list(&state, owner, Kind::Planner).await?))
}

#[derive(Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ImportParams {
    /// Replace the session kept from the same moment (`created_at`, to the second), if there is one.
    /// Without it such an upload is refused with `409`.
    pub overwrite: Option<bool>,
}

#[utoipa::path(
    post,
    path = "/planner/recorded/import",
    operation_id = "import_plan_session",
    tag = tags::USER_PLANNER,
    summary = "Record a session from a downloaded one",
    description = "The body is a document from export_plan_session, or the JSON the planner and its viewer download (format \
                   `cairezeki-plan`, the session under `session`), as downloaded. It is recorded anew - \
                   read-only, counted against `plan_sessions_max` - and keeps its `created_at`, which identifies \
                   it: when a session from the same moment is kept already, the upload is refused with `409` \
                   unless `overwrite=true`, which replaces it, keeping its id. A name taken by another session \
                   gets ` (2)`, ` (3)`, ... `403` when recording or uploading is turned off \
                   (`plan_sessions_enabled`, `plan_sessions_upload`); `409` too when as many sessions are kept \
                   as allowed. Through MCP the file goes in `body`, as downloaded, and `overwrite` beside it.",
    params(ImportParams),
    request_body = PlanSessionUpload,
    responses(
        (status = 200, body = SessionList),
        (status = 400, description = "Not a downloaded session", body = crate::error::ErrorBody),
        (status = 403, description = "Recording or uploading is turned off", body = crate::error::ErrorBody),
        (status = 409, description = "A session from the same moment is kept already, or as many sessions as allowed", body = crate::error::ErrorBody)
    )
)]
pub async fn import_session(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Query(q): Query<ImportParams>,
    Json(body): Json<serde_json::Value>,
) -> AppResult<Json<SessionList>> {
    let doc = plan_sessions::session_of(Kind::Planner, body)?;
    plan_sessions::import(&state.db, owner, Kind::Planner, &doc, q.overwrite.unwrap_or(false)).await?;
    Ok(Json(session_list(&state, owner, Kind::Planner).await?))
}

#[utoipa::path(
    get,
    path = "/planner/recorded/{id}",
    operation_id = "get_plan_session",
    tag = tags::USER_PLANNER,
    summary = "One recorded planner session, with its plan",
    params(("id" = i32, Path, description = "The session's id")),
    responses((status = 200, body = PlanSession), (status = 404, body = crate::error::ErrorBody))
)]
pub async fn get_session(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<i32>,
) -> AppResult<Json<PlanSession>> {
    Ok(Json(plan_sessions::get(&state.db, owner, Kind::Planner, id).await?.try_into()?))
}

#[utoipa::path(
    get,
    path = "/planner/recorded/{id}/export",
    operation_id = "export_plan_session",
    tag = tags::USER_PLANNER,
    summary = "A recorded planner session as a JSON document to download",
    description = "import_plan_session takes it back. Over HTTP the file is named \
                   `<name>_<saved date and time, UTC>.json` in `Content-Disposition`; through MCP only the \
                   document comes back. `403` when downloading is turned off (`plan_sessions_download`), which \
                   governs this file alone: get_plan_session, which the planner page opens a session with, \
                   reads it whatever the setting.",
    params(("id" = i32, Path, description = "The session's id")),
    responses(
        (status = 200, body = PlanSessionDoc),
        (status = 403, description = "Downloading is turned off", body = crate::error::ErrorBody),
        (status = 404, body = crate::error::ErrorBody)
    )
)]
pub async fn export_session(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    let (doc, file) = plan_sessions::export(&state.db, owner, Kind::Planner, id).await?;
    Ok(([(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{file}\""))], Json(doc)).into_response())
}

#[utoipa::path(
    delete,
    path = "/planner/recorded/{id}",
    operation_id = "delete_plan_session",
    tag = tags::USER_PLANNER,
    summary = "Delete a recorded planner session",
    params(("id" = i32, Path, description = "The session's id")),
    responses((status = 200, body = SessionList), (status = 404, body = crate::error::ErrorBody))
)]
pub async fn delete_session(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<i32>,
) -> AppResult<Json<SessionList>> {
    plan_sessions::delete(&state.db, owner, Kind::Planner, id).await?;
    Ok(Json(session_list(&state, owner, Kind::Planner).await?))
}
