//! CaiYatra: the corpus from today, built up to retirement and drawn down after,
//! and the page's recorded sessions (kept as the planner's are; see
//! `services::plan_sessions`).

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::auth::Owner;
use crate::error::{AppError, AppResult};
use crate::services::dhan_yatra::{self, DhanYatraInput, DhanYatraPlan};
use crate::services::plan_sessions::{self, Kind};
use crate::state::AppState;

use super::openapi::tags;
use super::planner::{session_list, ImportParams, SessionList};

/// The CaiYatra screen's simulation table. Nothing is stored: it is
/// arithmetic on the portfolio and the assumptions given, so it is safe to
/// call as often as needed.
#[utoipa::path(
    post,
    path = "/dhan-yatra/simulate",
    operation_id = "simulate_dhan_yatra",
    tag = tags::USER_DHAN_YATRA,
    summary = "The journey of the corpus: built up to retirement, then drawn down - the simulation table",
    description = "Runs the corpus from today, a year at a time, once per withdrawal (Sim 1, Sim 2, ...). \
                   Until `retire_year` nothing comes out unless `pre_retirement` says so (`pct` of each year's \
                   opening balance, or an `amount` rising with inflation); from it, each Sim's withdrawal comes \
                   out, in today's money and rising with inflation. Each year: the withdrawal comes out and \
                   `extras` go in or out, then what is left earns the return, each currency's part moving \
                   against `currency` at its assumed drift. The first row is what is left of this year. \
                   `adjustments` move the return, inflation or withdrawal of any year or run of years off the \
                   plan, as the screen's rope does. Every field is optional - the corpus and its currency \
                   split default to today's net worth, inflation to the currency's ten-year average, \
                   retirement to age 60 (with `age`) or 15 years out, the withdrawals to 3, 4, 5 and 6% of the \
                   corpus at retirement - and `assumptions` in the answer says what was used. Each Sim's \
                   `rows` are the table: opening, withdrawal, additional, returns, currency impact, closing \
                   and closing in today's money, stopping at the first year that opens and closes below zero. \
                   Nothing is stored.",
    request_body(content = Option<DhanYatraInput>, content_type = "application/json"),
    responses((status = 200, body = DhanYatraPlan), (status = 400, body = crate::error::ErrorBody))
)]
pub async fn simulate(
    State(state): State<AppState>,
    Owner(owner): Owner,
    body: Bytes,
) -> AppResult<Json<DhanYatraPlan>> {
    // Read by hand rather than as `Option<Json<_>>`, which would quietly run
    // the defaults on a body that does not parse.
    let input = if body.iter().all(u8::is_ascii_whitespace) {
        DhanYatraInput::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| AppError::bad(format!("invalid body: {e}")))?
    };
    Ok(Json(dhan_yatra::simulate(&state, owner, input).await?))
}


// ---------------------------------------------------------------------------
// Recorded sessions
// ---------------------------------------------------------------------------

/// A recorded CaiYatra session in full.
#[derive(Serialize, utoipa::ToSchema)]
pub struct YatraSession {
    pub id: i32,
    pub name: String,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uploaded_at: Option<DateTime<Utc>>,
    /// The page as it was: `{version: 1, inputs, selected, real, portfolio}` - every input, the Sim
    /// picked, whether amounts were in today's money, and the portfolio it started from.
    #[schema(value_type = Object)]
    pub page: serde_json::Value,
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct NewYatraSession {
    /// 1 to 120 characters, unique among the caller's CaiYatra sessions (ignoring case).
    #[schema(example = "Retire at 55")]
    pub name: String,
    /// The page as it is: `{version: 1, inputs, ...}`. Only the web app fills it in.
    #[schema(value_type = Object)]
    pub page: serde_json::Value,
}

/// A CaiYatra session as export_dhan_yatra_session gives it, and as import_dhan_yatra_session takes
/// it back.
#[derive(Deserialize, utoipa::ToSchema)]
#[allow(dead_code)] // read as a `plan_sessions::SessionDoc`; this is its CaiYatra shape, for the docs
pub struct YatraSessionDoc {
    /// `cairezeki-dhan-yatra-session`.
    format: String,
    /// 1.
    version: u32,
    name: String,
    /// When the session was first saved, with its offset.
    created_at: DateTime<Utc>,
    /// When it was downloaded; ignored on upload.
    exported_at: Option<DateTime<Utc>>,
    /// The page as it was: `{version: 1, inputs, selected, real, portfolio}`.
    #[schema(value_type = Object)]
    page: serde_json::Value,
}

/// The CaiYatra page's own JSON download, as downloaded - signed in or not: the session under
/// `session`. Its other fields are not read.
#[derive(Deserialize, utoipa::ToSchema)]
#[allow(dead_code)]
pub struct YatraPageDownload {
    /// `cairezeki-dhan-yatra`.
    format: String,
    session: YatraSessionDoc,
}

/// A downloaded CaiYatra session, either way it is downloaded.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(untagged)]
#[allow(dead_code)]
pub enum YatraSessionUpload {
    Session(YatraSessionDoc),
    Page(YatraPageDownload),
}

#[utoipa::path(
    get,
    path = "/dhan-yatra/recorded",
    operation_id = "list_dhan_yatra_sessions",
    tag = tags::USER_DHAN_YATRA,
    summary = "The caller's recorded CaiYatra sessions, newest first, and what their settings allow",
    description = "A recorded session is the CaiYatra page as it stood at a moment - every input, and the \
                   portfolio it started from - under a name. It never changes once saved. `enabled`, `max`, \
                   `download` and `upload` are the caller's `yatra_sessions_*` settings.",
    responses((status = 200, body = SessionList))
)]
pub async fn list_sessions(State(state): State<AppState>, Owner(owner): Owner) -> AppResult<Json<SessionList>> {
    Ok(Json(session_list(&state, owner, Kind::DhanYatra).await?))
}

#[utoipa::path(
    post,
    path = "/dhan-yatra/recorded",
    operation_id = "save_dhan_yatra_session",
    tag = tags::USER_DHAN_YATRA,
    summary = "Record the CaiYatra page as it is",
    description = "What the page's Save does. `403` when recording is turned off (`yatra_sessions_enabled`); \
                   `409` when the name is taken or the caller keeps as many sessions as `yatra_sessions_max` \
                   allows.",
    request_body = NewYatraSession,
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
    Json(body): Json<NewYatraSession>,
) -> AppResult<Json<SessionList>> {
    plan_sessions::create(&state.db, owner, Kind::DhanYatra, &body.name, None, &body.page).await?;
    Ok(Json(session_list(&state, owner, Kind::DhanYatra).await?))
}

#[utoipa::path(
    post,
    path = "/dhan-yatra/recorded/import",
    operation_id = "import_dhan_yatra_session",
    tag = tags::USER_DHAN_YATRA,
    summary = "Record a CaiYatra session from a downloaded one",
    description = "The body is a document from export_dhan_yatra_session, or the page's own JSON download \
                   (format `cairezeki-dhan-yatra`, the session under `session`), as downloaded - the same file \
                   a visitor who is not signed in downloads and uploads. It is recorded anew - read-only, \
                   counted against `yatra_sessions_max` - and keeps its `created_at`, which identifies it: \
                   when a session from the same moment is kept already, the upload is refused with `409` \
                   unless `overwrite=true`, which replaces it, keeping its id. A name taken by another session \
                   gets ` (2)`, ` (3)`, ... A session with a `plan` is a planner session, refused with `400`. \
                   `403` when recording or uploading is turned off (`yatra_sessions_enabled`, \
                   `yatra_sessions_upload`); `409` too when as many sessions are kept as allowed. Through MCP \
                   the file goes in `body`, as downloaded, and `overwrite` beside it.",
    params(ImportParams),
    request_body = YatraSessionUpload,
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
    let doc = plan_sessions::session_of(Kind::DhanYatra, body)?;
    plan_sessions::import(&state.db, owner, Kind::DhanYatra, &doc, q.overwrite.unwrap_or(false)).await?;
    Ok(Json(session_list(&state, owner, Kind::DhanYatra).await?))
}

#[utoipa::path(
    get,
    path = "/dhan-yatra/recorded/{id}",
    operation_id = "get_dhan_yatra_session",
    tag = tags::USER_DHAN_YATRA,
    summary = "One recorded CaiYatra session, with every input",
    params(("id" = i32, Path, description = "The session's id")),
    responses((status = 200, body = YatraSession), (status = 404, body = crate::error::ErrorBody))
)]
pub async fn get_session(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<i32>,
) -> AppResult<Json<YatraSession>> {
    let m = plan_sessions::get(&state.db, owner, Kind::DhanYatra, id).await?;
    Ok(Json(YatraSession {
        id: m.id,
        created_at: m.created_at.and_utc(),
        uploaded_at: m.uploaded_at.map(|t| t.and_utc()),
        page: serde_json::from_str(&m.page).map_err(|e| AppError::Other(e.into()))?,
        name: m.name,
    }))
}

#[utoipa::path(
    get,
    path = "/dhan-yatra/recorded/{id}/export",
    operation_id = "export_dhan_yatra_session",
    tag = tags::USER_DHAN_YATRA,
    summary = "A recorded CaiYatra session as a JSON document to download",
    description = "import_dhan_yatra_session takes it back. Over HTTP the file is named \
                   `<name>_<saved date and time, UTC>.json` in `Content-Disposition`; through MCP only the \
                   document comes back. `403` when downloading is turned off (`yatra_sessions_download`), which \
                   governs this file alone: get_dhan_yatra_session, which the page opens a session with, reads \
                   it whatever the setting.",
    params(("id" = i32, Path, description = "The session's id")),
    responses(
        (status = 200, body = YatraSessionDoc),
        (status = 403, description = "Downloading is turned off", body = crate::error::ErrorBody),
        (status = 404, body = crate::error::ErrorBody)
    )
)]
pub async fn export_session(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<i32>,
) -> AppResult<Response> {
    let (doc, file) = plan_sessions::export(&state.db, owner, Kind::DhanYatra, id).await?;
    Ok(([(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{file}\""))], Json(doc)).into_response())
}

#[utoipa::path(
    delete,
    path = "/dhan-yatra/recorded/{id}",
    operation_id = "delete_dhan_yatra_session",
    tag = tags::USER_DHAN_YATRA,
    summary = "Delete a recorded CaiYatra session",
    params(("id" = i32, Path, description = "The session's id")),
    responses((status = 200, body = SessionList), (status = 404, body = crate::error::ErrorBody))
)]
pub async fn delete_session(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<i32>,
) -> AppResult<Json<SessionList>> {
    plan_sessions::delete(&state.db, owner, Kind::DhanYatra, id).await?;
    Ok(Json(session_list(&state, owner, Kind::DhanYatra).await?))
}
