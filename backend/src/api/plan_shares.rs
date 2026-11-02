//! Sharing saved planner sessions, read-only: with another user, or as a
//! link anyone may open - see `services::plan_shares`.

use axum::extract::{Path, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::auth::Owner;
use crate::error::AppResult;
use crate::services::plan_shares::{self, Audience, Share};
use crate::state::AppState;

use super::openapi::tags;
use super::planner::PlanSession;

/// The recipient of a share with a user.
#[derive(Serialize, utoipa::ToSchema)]
pub struct ShareRecipient {
    pub name: String,
    pub email: String,
}

/// A share, as its owner sees it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PlanShareView {
    pub id: i32,
    /// `user`: with another user. `link`: anyone with the link.
    #[schema(example = "link")]
    pub kind: String,
    /// Whom a `user` share is with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipient: Option<ShareRecipient>,
    /// A link's page in the web app, `/shared/<token>`: put the app's address in front of it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The owner's own name for it; may be empty.
    pub label: String,
    pub created_at: DateTime<Utc>,
    /// When it stops opening; absent: until revoked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    /// Past `expires_at`: it opens for nobody, and can be given a new expiry.
    pub expired: bool,
    /// The ids of the saved planner sessions it shows.
    pub sessions: Vec<i32>,
}

impl From<Share> for PlanShareView {
    fn from(s: Share) -> Self {
        let expired = s.expired();
        PlanShareView {
            id: s.row.id,
            kind: s.row.kind,
            recipient: s.recipient.map(|(name, email)| ShareRecipient { name, email }),
            path: s.row.token.map(|t| format!("/shared/{t}")),
            label: s.row.label,
            created_at: s.row.created_at.and_utc(),
            expires_at: s.row.expires_at.map(|t| t.and_utc()),
            expired,
            sessions: s.session_ids,
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PlanShareList {
    /// Newest first.
    pub shares: Vec<PlanShareView>,
    /// How many shares the caller may hold, people and links together.
    pub max: u64,
}

async fn share_list(state: &AppState, owner: i32) -> AppResult<PlanShareList> {
    Ok(PlanShareList {
        shares: plan_shares::list(&state.db, owner).await?.into_iter().map(Into::into).collect(),
        max: plan_shares::MAX_SHARES,
    })
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ShareKind {
    User,
    Link,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewPlanShare {
    /// `user`: with another user, by `email`. `link`: anyone with the link, without signing in.
    pub kind: ShareKind,
    /// The email address the other user signs in with; for `user` only.
    #[serde(default)]
    pub email: Option<String>,
    /// Ids of the caller's saved planner sessions to share (list_plan_sessions); at least one.
    pub sessions: Vec<i32>,
    /// A name for it, up to 120 characters.
    #[serde(default)]
    pub label: Option<String>,
    /// When it stops opening, in the future; left out: until revoked.
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatePlanShare {
    /// The saved planner sessions it shows from now on, in place of those it showed; at least one.
    pub sessions: Vec<i32>,
    #[serde(default)]
    pub label: Option<String>,
    /// When it stops opening; left out or `null`: until revoked.
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

#[utoipa::path(
    get,
    path = "/planner/shares",
    operation_id = "list_plan_shares",
    tag = tags::USER_PLANNER,
    summary = "The caller's shares of their saved planner sessions: with users, and links",
    description = "Each share shows a set of the caller's saved planner sessions, read-only - to one other user, \
                   or to anyone with its link. `sessions` are the ids it shows; a link's page is `path`.",
    responses((status = 200, body = PlanShareList))
)]
pub async fn list(State(state): State<AppState>, Owner(owner): Owner) -> AppResult<Json<PlanShareList>> {
    Ok(Json(share_list(&state, owner).await?))
}

#[utoipa::path(
    post,
    path = "/planner/shares",
    operation_id = "create_plan_share",
    tag = tags::USER_PLANNER,
    summary = "Share saved planner sessions with a user, or as a link",
    description = "`kind=user` shares them with the user who signs in with `email`: they see them under \
                   \"Shared with me\" on their planner, read-only, with the caller's name. One share per \
                   user - change its sessions with update_plan_share. `kind=link` makes a link anyone can \
                   open without signing in (`path` in the answer): a page with the planner alone, read-only, \
                   showing only these sessions. Only saved sessions are shared, as they were saved; never the \
                   live plan. The web app only: a link reaches whoever has it, so it is not a tool.",
    request_body = NewPlanShare,
    responses(
        (status = 200, body = PlanShareView),
        (status = 400, body = crate::error::ErrorBody),
        (status = 404, description = "No such user, or a session that is not the caller's", body = crate::error::ErrorBody),
        (status = 409, description = "Shared with that user already, or as many shares held as allowed", body = crate::error::ErrorBody)
    )
)]
pub async fn create(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Json(body): Json<NewPlanShare>,
) -> AppResult<Json<PlanShareView>> {
    let audience = match body.kind {
        ShareKind::User => Audience::User(body.email.as_deref().unwrap_or("")),
        ShareKind::Link => {
            if body.email.as_deref().is_some_and(|e| !e.trim().is_empty()) {
                return Err(crate::error::AppError::bad("a link is for anyone: leave out email"));
            }
            Audience::Link
        }
    };
    let share =
        plan_shares::create(&state.db, owner, audience, &body.sessions, body.label.as_deref(), body.expires_at).await?;
    Ok(Json(share.into()))
}

#[utoipa::path(
    put,
    path = "/planner/shares/{id}",
    operation_id = "update_plan_share",
    tag = tags::USER_PLANNER,
    summary = "Change which saved sessions a share shows, its label and its expiry",
    description = "Replaces all three. A link keeps its URL. The web app only, as create_plan_share.",
    params(("id" = i32, Path, description = "The share's id")),
    request_body = UpdatePlanShare,
    responses(
        (status = 200, body = PlanShareView),
        (status = 400, body = crate::error::ErrorBody),
        (status = 404, body = crate::error::ErrorBody)
    )
)]
pub async fn update(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<i32>,
    Json(body): Json<UpdatePlanShare>,
) -> AppResult<Json<PlanShareView>> {
    let share =
        plan_shares::update(&state.db, owner, id, &body.sessions, body.label.as_deref(), body.expires_at).await?;
    Ok(Json(share.into()))
}

#[utoipa::path(
    delete,
    path = "/planner/shares/{id}",
    operation_id = "revoke_plan_share",
    tag = tags::USER_PLANNER,
    summary = "Revoke a share: the user no longer sees the sessions, the link opens for nobody",
    params(("id" = i32, Path, description = "The share's id")),
    responses((status = 200, body = PlanShareList), (status = 404, body = crate::error::ErrorBody))
)]
pub async fn revoke(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<i32>,
) -> AppResult<Json<PlanShareList>> {
    plan_shares::revoke(&state.db, owner, id).await?;
    Ok(Json(share_list(&state, owner).await?))
}

/// A saved planner session someone shares, without its contents.
#[derive(Serialize, utoipa::ToSchema)]
pub struct SharedSessionListed {
    pub id: i32,
    pub name: String,
    /// When it was first saved, UTC.
    pub created_at: DateTime<Utc>,
    /// Whose it is: their display name.
    pub owner_name: String,
}

impl From<plan_shares::SharedListed> for SharedSessionListed {
    fn from(s: plan_shares::SharedListed) -> Self {
        SharedSessionListed {
            id: s.session.id,
            name: s.session.name,
            created_at: s.session.created_at.and_utc(),
            owner_name: s.owner_name,
        }
    }
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct SharedSessionList {
    /// Newest first.
    pub sessions: Vec<SharedSessionListed>,
}

/// A saved planner session someone shares, in full: as it was saved, and whose it is.
#[derive(Serialize, utoipa::ToSchema)]
pub struct SharedPlanSession {
    pub id: i32,
    pub name: String,
    pub created_at: DateTime<Utc>,
    /// The plan as it stood when saved.
    pub plan: crate::services::planner::Plan,
    /// The values the planner page showed the plan against, in the page's own shape.
    #[schema(value_type = Object)]
    pub page: serde_json::Value,
    /// Whose it is: their display name.
    pub owner_name: String,
}

fn shared_session(m: crate::entities::plan_session::Model, owner_name: String) -> AppResult<SharedPlanSession> {
    let s = PlanSession::try_from(m)?;
    Ok(SharedPlanSession { id: s.id, name: s.name, created_at: s.created_at, plan: s.plan, page: s.page, owner_name })
}

#[utoipa::path(
    get,
    path = "/planner/shared",
    operation_id = "list_plans_shared_with_me",
    tag = tags::USER_PLANNER,
    summary = "Saved planner sessions other users share with the caller",
    description = "Read-only, as their owners saved them; the planner lists them under \"Shared with me\".",
    responses((status = 200, body = SharedSessionList))
)]
pub async fn shared_with_me(State(state): State<AppState>, Owner(owner): Owner) -> AppResult<Json<SharedSessionList>> {
    let sessions = plan_shares::shared_with(&state.db, owner).await?.into_iter().map(Into::into).collect();
    Ok(Json(SharedSessionList { sessions }))
}

#[utoipa::path(
    get,
    path = "/planner/shared/{id}",
    operation_id = "get_plan_shared_with_me",
    tag = tags::USER_PLANNER,
    summary = "One saved planner session shared with the caller, with its plan",
    params(("id" = i32, Path, description = "The session's id, from list_plans_shared_with_me")),
    responses((status = 200, body = SharedPlanSession), (status = 404, body = crate::error::ErrorBody))
)]
pub async fn get_shared_with_me(
    State(state): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<i32>,
) -> AppResult<Json<SharedPlanSession>> {
    let (m, owner_name) = plan_shares::get_shared_with(&state.db, owner, id).await?;
    Ok(Json(shared_session(m, owner_name)?))
}

/// What a link opens: whose it is, and the saved planner sessions it shows.
#[derive(Serialize, utoipa::ToSchema)]
pub struct SharedLink {
    /// Its owner's display name.
    pub owner_name: String,
    /// The owner's name for the link; may be empty.
    pub label: String,
    /// When it stops opening; absent: until revoked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    /// Newest first.
    pub sessions: Vec<SharedSessionListed>,
}

/// Nothing a link answers may be kept by a cache, indexed, or passed on in a `Referer`.
fn private<T: Serialize>(body: T) -> Response {
    (
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::REFERRER_POLICY, "no-referrer"),
            (header::HeaderName::from_static("x-robots-tag"), "noindex, nofollow"),
        ],
        Json(body),
    )
        .into_response()
}

#[utoipa::path(
    get,
    path = "/shared/{token}",
    operation_id = "open_shared_link",
    tag = tags::PUBLIC_SHARED,
    summary = "What a shared link shows: the saved planner sessions in it",
    description = "No sign-in: the link is the credential. `404` for a link that is unknown, revoked or expired, \
                   all alike.",
    params(("token" = String, Path, description = "The last part of the link")),
    responses((status = 200, body = SharedLink), (status = 404, body = crate::error::ErrorBody))
)]
pub async fn open_link(State(state): State<AppState>, Path(token): Path<String>) -> AppResult<Response> {
    let (share, owner_name) = plan_shares::link(&state.db, &token).await?;
    let sessions = plan_shares::link_sessions(&state.db, &share).await?.into_iter().map(Into::into).collect();
    Ok(private(SharedLink {
        owner_name,
        label: share.label,
        expires_at: share.expires_at.map(|t| t.and_utc()),
        sessions,
    }))
}

#[utoipa::path(
    get,
    path = "/shared/{token}/plans/{id}",
    operation_id = "get_shared_link_plan",
    tag = tags::PUBLIC_SHARED,
    summary = "One saved planner session a shared link shows, with its plan",
    description = "No sign-in: the link is the credential. Only the sessions in the link open; any other, like a \
                   link that is unknown, revoked or expired, is `404`.",
    params(
        ("token" = String, Path, description = "The last part of the link"),
        ("id" = i32, Path, description = "The session's id, from open_shared_link")
    ),
    responses((status = 200, body = SharedPlanSession), (status = 404, body = crate::error::ErrorBody))
)]
pub async fn link_plan(State(state): State<AppState>, Path((token, id)): Path<(String, i32)>) -> AppResult<Response> {
    let (share, owner_name) = plan_shares::link(&state.db, &token).await?;
    let m = plan_shares::link_session(&state.db, &share, id).await?;
    Ok(private(shared_session(m, owner_name)?))
}
