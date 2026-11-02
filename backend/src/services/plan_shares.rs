//! Sharing saved planner sessions, read-only.
//!
//! A user shares a set of their saved planner sessions (see
//! [`super::plan_sessions`]) either with another user, by email, or as a
//! link anyone may open without signing in. Each share has its own set,
//! which its owner can change at any time without changing the link; a
//! share is revoked by deleting it, and may be given a moment it stops
//! opening (`expires_at`).
//!
//! What a share shows is only ever the sessions in its set, as they were
//! saved: the plan and the values the page showed it against. Never the
//! owner's live plan or portfolio, nothing to change, and nothing else of
//! theirs. A user sees what is shared with them beside their own sessions,
//! under the owner's name; a link opens a page of its own.
//!
//! A link's secret is 32 random bytes, kept as it is so the owner can copy
//! the link again. It is no weaker for that: whoever can read the database
//! can read the sessions themselves. Anything wrong with a link - unknown,
//! revoked, expired, or a session not in its set - is the same "not found",
//! so a link says nothing about any other.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, NaiveDateTime, Utc};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, Condition, DatabaseConnection, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, TransactionTrait,
};

use crate::auth::{tokens, Role};
use crate::entities::{app_user, plan_session, plan_share, plan_share_session};
use crate::error::{AppError, AppResult};

use super::plan_sessions::{self, Kind};
use super::users;

/// Shares a user may hold: people and links together.
pub const MAX_SHARES: u64 = 50;
const MAX_LABEL: usize = 120;
/// Bytes of randomness in a link's secret.
const TOKEN_BYTES: usize = 32;

pub const USER: &str = "user";
pub const LINK: &str = "link";

/// Whom a new share is for.
pub enum Audience<'a> {
    /// Another user, by the email they sign in with.
    User(&'a str),
    Link,
}

/// A share as its owner sees it.
pub struct Share {
    pub row: plan_share::Model,
    /// The recipient of a user share: `(display name, email)`.
    pub recipient: Option<(String, String)>,
    /// Its sessions, by id, in order.
    pub session_ids: Vec<i32>,
}

impl Share {
    pub fn expired(&self) -> bool {
        is_expired(&self.row)
    }
}

fn is_expired(row: &plan_share::Model) -> bool {
    row.expires_at.is_some_and(|at| at <= Utc::now().naive_utc())
}

/// `expires_at` as asked for: in the future, or none.
fn expiry(at: Option<DateTime<Utc>>) -> AppResult<Option<NaiveDateTime>> {
    match at {
        Some(at) if at <= Utc::now() => Err(AppError::bad("expires_at must be in the future, or left out for no expiry")),
        at => Ok(at.map(|t| t.naive_utc())),
    }
}

fn label_of(label: Option<&str>) -> AppResult<String> {
    let label = label.unwrap_or("").trim().to_string();
    if label.chars().count() > MAX_LABEL {
        return Err(AppError::bad(format!("a share's label is at most {MAX_LABEL} characters")));
    }
    Ok(label)
}

/// The sessions asked for, each one of `owner`'s saved planner sessions; at least one.
async fn owned_sessions(db: &DatabaseConnection, owner: i32, ids: &[i32]) -> AppResult<Vec<i32>> {
    let mut seen = BTreeSet::new();
    let ids: Vec<i32> = ids.iter().copied().filter(|id| seen.insert(*id)).collect();
    if ids.is_empty() {
        return Err(AppError::bad("pick at least one saved planner session to share"));
    }
    let mine: BTreeSet<i32> = plan_sessions::list(db, owner, Kind::Planner).await?.into_iter().map(|s| s.id).collect();
    if let Some(id) = ids.iter().find(|id| !mine.contains(id)) {
        return Err(AppError::not_found("saved planner session", id));
    }
    Ok(ids)
}

/// Share `session_ids` of `owner`'s saved planner sessions with `audience`.
pub async fn create(
    db: &DatabaseConnection,
    owner: i32,
    audience: Audience<'_>,
    session_ids: &[i32],
    label: Option<&str>,
    expires_at: Option<DateTime<Utc>>,
) -> AppResult<Share> {
    let label = label_of(label)?;
    let expires_at = expiry(expires_at)?;
    let held = plan_share::Entity::find().filter(plan_share::Column::OwnerId.eq(owner)).count(db).await?;
    if held >= MAX_SHARES {
        return Err(AppError::Conflict(format!("you hold {MAX_SHARES} shares, the most allowed - revoke one first")));
    }
    let (kind, recipient, token) = match audience {
        Audience::User(email) => {
            let to = recipient(db, owner, email).await?;
            let already = plan_share::Entity::find()
                .filter(plan_share::Column::OwnerId.eq(owner))
                .filter(plan_share::Column::RecipientId.eq(to.id))
                .one(db)
                .await?;
            if already.is_some() {
                return Err(AppError::Conflict(format!(
                    "your plans are shared with {} already - change which plans that share has instead",
                    to.email
                )));
            }
            (USER, Some(to.id), None)
        }
        Audience::Link => (LINK, None, Some(tokens::random(TOKEN_BYTES))),
    };
    let ids = owned_sessions(db, owner, session_ids).await?;

    let tx = db.begin().await?;
    let row = plan_share::ActiveModel {
        owner_id: Set(owner),
        kind: Set(kind.to_string()),
        recipient_id: Set(recipient),
        token: Set(token),
        label: Set(label),
        created_at: Set(Utc::now().naive_utc()),
        expires_at: Set(expires_at),
        ..Default::default()
    }
    .insert(&tx)
    .await?;
    put_sessions(&tx, row.id, &ids).await?;
    tx.commit().await?;
    get(db, owner, row.id).await
}

/// Whom `email` names, if a share may go to them: another enabled user.
async fn recipient(db: &DatabaseConnection, owner: i32, email: &str) -> AppResult<app_user::Model> {
    let email = email.trim();
    if email.is_empty() {
        return Err(AppError::bad("give the email address of the user to share with"));
    }
    // Who has an account is not said: any address that is not a user's is the same "not found".
    let not_a_user = || AppError::NotFound(format!("no user signs in as {email}"));
    let to = users::find_by_email(db, email).await?.ok_or_else(not_a_user)?;
    if to.role != Role::User.as_str() || !to.is_enabled {
        return Err(not_a_user());
    }
    if to.id == owner {
        return Err(AppError::bad("that is you - share with someone else"));
    }
    Ok(to)
}

async fn put_sessions<C: sea_orm::ConnectionTrait>(db: &C, share_id: i32, ids: &[i32]) -> AppResult<()> {
    for id in ids {
        plan_share_session::ActiveModel { share_id: Set(share_id), session_id: Set(*id), ..Default::default() }
            .insert(db)
            .await?;
    }
    Ok(())
}

/// Change what one of `owner`'s shares shows, its label and its expiry. A link keeps its URL.
pub async fn update(
    db: &DatabaseConnection,
    owner: i32,
    id: i32,
    session_ids: &[i32],
    label: Option<&str>,
    expires_at: Option<DateTime<Utc>>,
) -> AppResult<Share> {
    let row = owned(db, owner, id).await?;
    let label = label_of(label)?;
    let expires_at = expiry(expires_at)?;
    let ids = owned_sessions(db, owner, session_ids).await?;
    let tx = db.begin().await?;
    plan_share::ActiveModel { id: Set(row.id), label: Set(label), expires_at: Set(expires_at), ..Default::default() }
        .update(&tx)
        .await?;
    plan_share_session::Entity::delete_many()
        .filter(plan_share_session::Column::ShareId.eq(row.id))
        .exec(&tx)
        .await?;
    put_sessions(&tx, row.id, &ids).await?;
    tx.commit().await?;
    get(db, owner, id).await
}

/// One of `owner`'s shares, bare. Someone else's is "not found".
async fn owned(db: &DatabaseConnection, owner: i32, id: i32) -> AppResult<plan_share::Model> {
    plan_share::Entity::find_by_id(id)
        .filter(plan_share::Column::OwnerId.eq(owner))
        .one(db)
        .await?
        .ok_or_else(|| AppError::not_found("share", id))
}

pub async fn get(db: &DatabaseConnection, owner: i32, id: i32) -> AppResult<Share> {
    let row = owned(db, owner, id).await?;
    let mut all = described(db, vec![row]).await?;
    Ok(all.remove(0))
}

/// `owner`'s shares, newest first.
pub async fn list(db: &DatabaseConnection, owner: i32) -> AppResult<Vec<Share>> {
    let rows = plan_share::Entity::find()
        .filter(plan_share::Column::OwnerId.eq(owner))
        .order_by_desc(plan_share::Column::CreatedAt)
        .order_by_desc(plan_share::Column::Id)
        .all(db)
        .await?;
    described(db, rows).await
}

/// Shares with their sessions and recipients filled in.
async fn described(db: &DatabaseConnection, rows: Vec<plan_share::Model>) -> AppResult<Vec<Share>> {
    let ids: Vec<i32> = rows.iter().map(|r| r.id).collect();
    let mut sessions: BTreeMap<i32, Vec<i32>> = BTreeMap::new();
    for s in plan_share_session::Entity::find()
        .filter(plan_share_session::Column::ShareId.is_in(ids))
        .order_by_asc(plan_share_session::Column::Id)
        .all(db)
        .await?
    {
        sessions.entry(s.share_id).or_default().push(s.session_id);
    }
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let recipient = match row.recipient_id {
            Some(uid) => app_user::Entity::find_by_id(uid).one(db).await?.map(|u| (u.display_name, u.email)),
            None => None,
        };
        out.push(Share { session_ids: sessions.remove(&row.id).unwrap_or_default(), recipient, row });
    }
    Ok(out)
}

/// Revoke one of `owner`'s shares: it opens for nobody from now on.
pub async fn revoke(db: &DatabaseConnection, owner: i32, id: i32) -> AppResult<()> {
    let row = owned(db, owner, id).await?;
    let tx = db.begin().await?;
    plan_share_session::Entity::delete_many().filter(plan_share_session::Column::ShareId.eq(row.id)).exec(&tx).await?;
    plan_share::Entity::delete_by_id(row.id).exec(&tx).await?;
    tx.commit().await?;
    Ok(())
}

/// Take a session out of every share, before it is deleted.
pub async fn forget_session(db: &DatabaseConnection, session_id: i32) -> AppResult<()> {
    plan_share_session::Entity::delete_many()
        .filter(plan_share_session::Column::SessionId.eq(session_id))
        .exec(db)
        .await?;
    Ok(())
}

/// Every share by or with `user_id`, before the account is deleted.
pub async fn forget_user<C: sea_orm::ConnectionTrait>(db: &C, user_id: i32) -> AppResult<()> {
    let shares: Vec<i32> = plan_share::Entity::find()
        .filter(
            Condition::any()
                .add(plan_share::Column::OwnerId.eq(user_id))
                .add(plan_share::Column::RecipientId.eq(user_id)),
        )
        .all(db)
        .await?
        .into_iter()
        .map(|s| s.id)
        .collect();
    if shares.is_empty() {
        return Ok(());
    }
    plan_share_session::Entity::delete_many()
        .filter(plan_share_session::Column::ShareId.is_in(shares.clone()))
        .exec(db)
        .await?;
    plan_share::Entity::delete_many().filter(plan_share::Column::Id.is_in(shares)).exec(db).await?;
    Ok(())
}

/// A session shared with someone, without its contents.
pub struct SharedListed {
    pub session: plan_sessions::Listed,
    /// The owner's display name.
    pub owner_name: String,
}

/// Sessions in `shares`, newest first, with their owners' names.
async fn sessions_of(db: &DatabaseConnection, shares: &[plan_share::Model]) -> AppResult<Vec<SharedListed>> {
    let mut owner_of: BTreeMap<i32, i32> = BTreeMap::new();
    for link in plan_share_session::Entity::find()
        .filter(plan_share_session::Column::ShareId.is_in(shares.iter().map(|s| s.id).collect::<Vec<_>>()))
        .all(db)
        .await?
    {
        if let Some(share) = shares.iter().find(|s| s.id == link.share_id) {
            owner_of.insert(link.session_id, share.owner_id);
        }
    }
    let rows = plan_session::Entity::find()
        .filter(plan_session::Column::Id.is_in(owner_of.keys().copied().collect::<Vec<_>>()))
        .filter(plan_session::Column::Kind.eq("planner"))
        .order_by_desc(plan_session::Column::CreatedAt)
        .order_by_desc(plan_session::Column::Id)
        .all(db)
        .await?;
    let mut names: BTreeMap<i32, String> = BTreeMap::new();
    let mut out = Vec::with_capacity(rows.len());
    for s in rows {
        // The share's owner must still own it: a session is only ever shown by whoever saved it.
        if owner_of.get(&s.id) != Some(&s.user_id) {
            continue;
        }
        if !names.contains_key(&s.user_id) {
            let name = app_user::Entity::find_by_id(s.user_id).one(db).await?.map(|u| u.display_name).unwrap_or_default();
            names.insert(s.user_id, name);
        }
        out.push(SharedListed {
            owner_name: names[&s.user_id].clone(),
            session: plan_sessions::Listed { id: s.id, name: s.name, created_at: s.created_at, uploaded_at: s.uploaded_at },
        });
    }
    Ok(out)
}

/// `user_id`'s shares from others that are still open.
async fn shares_with(db: &DatabaseConnection, user_id: i32) -> AppResult<Vec<plan_share::Model>> {
    Ok(plan_share::Entity::find()
        .filter(plan_share::Column::Kind.eq(USER))
        .filter(plan_share::Column::RecipientId.eq(user_id))
        .all(db)
        .await?
        .into_iter()
        .filter(|s| !is_expired(s))
        .collect())
}

/// The sessions other users share with `user_id`.
pub async fn shared_with(db: &DatabaseConnection, user_id: i32) -> AppResult<Vec<SharedListed>> {
    let shares = shares_with(db, user_id).await?;
    sessions_of(db, &shares).await
}

/// A session shared with `user_id`, and its owner's name. Anything else is "not found".
pub async fn get_shared_with(
    db: &DatabaseConnection,
    user_id: i32,
    session_id: i32,
) -> AppResult<(plan_session::Model, String)> {
    let shares = shares_with(db, user_id).await?;
    one_of(db, &shares, session_id).await
}

/// `session_id`, if one of `shares` holds it and its owner still owns it.
async fn one_of(
    db: &DatabaseConnection,
    shares: &[plan_share::Model],
    session_id: i32,
) -> AppResult<(plan_session::Model, String)> {
    let gone = || AppError::not_found("shared plan", session_id);
    let held = plan_share_session::Entity::find()
        .filter(plan_share_session::Column::SessionId.eq(session_id))
        .filter(plan_share_session::Column::ShareId.is_in(shares.iter().map(|s| s.id).collect::<Vec<_>>()))
        .all(db)
        .await?;
    let owners: BTreeSet<i32> =
        held.iter().filter_map(|h| shares.iter().find(|s| s.id == h.share_id)).map(|s| s.owner_id).collect();
    let session = plan_session::Entity::find_by_id(session_id)
        .filter(plan_session::Column::Kind.eq("planner"))
        .one(db)
        .await?
        .filter(|s| owners.contains(&s.user_id))
        .ok_or_else(gone)?;
    let owner = app_user::Entity::find_by_id(session.user_id).one(db).await?.map(|u| u.display_name).unwrap_or_default();
    Ok((session, owner))
}

/// An open link: its share and its owner's name. Unknown, revoked or expired is "not found".
pub async fn link(db: &DatabaseConnection, token: &str) -> AppResult<(plan_share::Model, String)> {
    let gone = || AppError::NotFound("this link is not valid - it may have been revoked or have expired".into());
    let token = token.trim();
    if token.is_empty() || token.len() > 64 {
        return Err(gone());
    }
    let share = plan_share::Entity::find()
        .filter(plan_share::Column::Kind.eq(LINK))
        .filter(plan_share::Column::Token.eq(token))
        .one(db)
        .await?
        .filter(|s| !is_expired(s))
        .ok_or_else(gone)?;
    let owner = app_user::Entity::find_by_id(share.owner_id).one(db).await?.ok_or_else(gone)?;
    if !owner.is_enabled {
        return Err(gone());
    }
    Ok((share, owner.display_name))
}

/// The sessions a link shows, newest first.
pub async fn link_sessions(db: &DatabaseConnection, share: &plan_share::Model) -> AppResult<Vec<SharedListed>> {
    sessions_of(db, std::slice::from_ref(share)).await
}

/// One of the sessions a link shows. Any other is "not found".
pub async fn link_session(db: &DatabaseConnection, share: &plan_share::Model, session_id: i32) -> AppResult<plan_session::Model> {
    Ok(one_of(db, std::slice::from_ref(share), session_id).await?.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_is_trimmed_and_bounded() {
        assert_eq!(label_of(Some("  For Ravi ")).unwrap(), "For Ravi");
        assert_eq!(label_of(None).unwrap(), "");
        assert!(label_of(Some(&"x".repeat(MAX_LABEL + 1))).is_err());
    }

    #[test]
    fn an_expiry_is_in_the_future_or_none() {
        assert!(expiry(None).unwrap().is_none());
        assert!(expiry(Some(Utc::now() + chrono::Duration::days(1))).unwrap().is_some());
        assert!(expiry(Some(Utc::now() - chrono::Duration::seconds(1))).is_err());
    }

    #[test]
    fn a_link_secret_is_long_and_url_safe() {
        let t = tokens::random(TOKEN_BYTES);
        assert_eq!(t.len(), 43);
        assert!(t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'), "{t}");
    }
}
