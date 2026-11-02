//! Recorded sessions: of RezekiYojana (the planner) and of CaiYatra.
//!
//! The planner's plan is always worked out against today's values, and Dhan
//! Yatra starts from today's portfolio, so both read differently as prices
//! move. A recorded session keeps what the page showed - for the planner the
//! plan and the values it was shown against, for CaiYatra every input and
//! the portfolio it started from (`page`, the page's own JSON) - under a
//! name and the moment it was saved, so it can be looked at later exactly as
//! it was. A session is never changed once saved: it is kept until its
//! owner deletes it. Names are unique per user and page, ignoring case.
//!
//! A session can be downloaded as a JSON document ([`SessionDoc`]) and
//! uploaded again - here or on another account - which records it anew,
//! keeping the moment it was first saved. That moment identifies it: an
//! upload of a session kept already replaces it only when asked to. Each
//! page's own JSON download carries the session too, under `session`, so
//! one file serves both (the planner's beside what the plan viewer shows).
//!
//! Each user decides, in their profile and for each page, whether sessions
//! are recorded at all (`<prefix>_enabled`), how many are kept
//! (`<prefix>_max`, [`DEFAULT_MAX`] when not set), and whether they may be
//! downloaded (`<prefix>_download`) and uploaded (`<prefix>_upload`); the
//! prefix is `plan_sessions` or `yatra_sessions`, and the switches are on
//! unless turned off. Once as many are kept as allowed, recording another
//! is refused until one is deleted - nothing is ever thrown away unasked.
//! Turning recording off keeps the sessions already recorded, to look at
//! and delete.

use chrono::{DateTime, NaiveDateTime, SubsecRound, Utc};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect,
};
use serde::{Deserialize, Serialize};

use crate::entities::plan_session;
use crate::error::{AppError, AppResult};

use super::planner::{self, Plan};
use super::prefs;

/// Sessions a user may keep when they have not said otherwise.
pub const DEFAULT_MAX: u32 = 10;
/// The most `<prefix>_max` may be set to.
pub const MOST_ALLOWED: u32 = 100;
pub const DOC_VERSION: u32 = 1;
/// The page's values are bounded by the request size; this keeps one row sane.
const MAX_PAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_NAME: usize = 120;

/// The on/off preferences, `true` or `false`.
pub const SWITCHES: &[&str] = &[
    "plan_sessions_enabled",
    "plan_sessions_download",
    "plan_sessions_upload",
    "yatra_sessions_enabled",
    "yatra_sessions_download",
    "yatra_sessions_upload",
];
/// How many sessions to keep, 1 to [`MOST_ALLOWED`].
pub const MAXES: &[&str] = &["plan_sessions_max", "yatra_sessions_max"];

/// Which page a session is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Planner,
    DhanYatra,
}

impl Kind {
    /// As kept in `plan_session.kind`.
    fn key(self) -> &'static str {
        match self {
            Kind::Planner => "planner",
            Kind::DhanYatra => "dhan_yatra",
        }
    }
    /// What its settings are called, before `_enabled`, `_max`, ...
    fn prefix(self) -> &'static str {
        match self {
            Kind::Planner => "plan_sessions",
            Kind::DhanYatra => "yatra_sessions",
        }
    }
    /// For messages: "planner" or "CaiYatra".
    fn label(self) -> &'static str {
        match self {
            Kind::Planner => "planner",
            Kind::DhanYatra => "CaiYatra",
        }
    }
    /// What a downloaded session says it is.
    pub fn doc_format(self) -> &'static str {
        match self {
            Kind::Planner => "cairezeki-plan-session",
            Kind::DhanYatra => "cairezeki-dhan-yatra-session",
        }
    }
    /// What the page's own JSON download says it is: it carries a session under `session`.
    fn page_format(self) -> &'static str {
        match self {
            Kind::Planner => "cairezeki-plan",
            Kind::DhanYatra => "cairezeki-dhan-yatra",
        }
    }
    /// Its card in the profile.
    fn card(self) -> &'static str {
        match self {
            Kind::Planner => "Planner",
            Kind::DhanYatra => "CaiYatra",
        }
    }
}

/// `<prefix>_max` as set, if it is a valid one.
pub fn parse_max(value: &str) -> Option<u32> {
    value.trim().parse::<u32>().ok().filter(|n| (1..=MOST_ALLOWED).contains(n))
}

/// What a user allows of a page's recorded sessions.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Sessions may be recorded (saved and uploaded).
    pub enabled: bool,
    /// How many may be kept.
    pub max: u32,
    /// Sessions may be downloaded as JSON.
    pub download: bool,
    /// Downloaded sessions may be uploaded again.
    pub upload: bool,
}

async fn pref(db: &DatabaseConnection, user_id: i32, key: String) -> Option<String> {
    prefs::get_opt(db, user_id, &key).await
}

pub async fn options(db: &DatabaseConnection, user_id: i32, kind: Kind) -> Options {
    let p = kind.prefix();
    let get = |s: &str| pref(db, user_id, format!("{p}_{s}"));
    let on = |v: Option<String>| v.map_or(true, |v| v != "false");
    Options {
        enabled: on(get("enabled").await),
        max: get("max").await.and_then(|v| parse_max(&v)).unwrap_or(DEFAULT_MAX),
        download: on(get("download").await),
        upload: on(get("upload").await),
    }
}

/// A session without its contents, for the list.
pub struct Listed {
    pub id: i32,
    pub name: String,
    pub created_at: NaiveDateTime,
    pub uploaded_at: Option<NaiveDateTime>,
}

/// `user_id`'s sessions of a page, newest first.
pub async fn list(db: &DatabaseConnection, user_id: i32, kind: Kind) -> AppResult<Vec<Listed>> {
    Ok(plan_session::Entity::find()
        .select_only()
        .column(plan_session::Column::Id)
        .column(plan_session::Column::Name)
        .column(plan_session::Column::CreatedAt)
        .column(plan_session::Column::UploadedAt)
        .filter(plan_session::Column::UserId.eq(user_id))
        .filter(plan_session::Column::Kind.eq(kind.key()))
        .order_by_desc(plan_session::Column::CreatedAt)
        .order_by_desc(plan_session::Column::Id)
        .into_tuple::<(i32, String, NaiveDateTime, Option<NaiveDateTime>)>()
        .all(db)
        .await?
        .into_iter()
        .map(|(id, name, created_at, uploaded_at)| Listed { id, name, created_at, uploaded_at })
        .collect())
}

/// A session as downloaded, and as uploaded again.
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SessionDoc {
    /// `cairezeki-plan-session` or `cairezeki-dhan-yatra-session`.
    pub format: String,
    /// 1.
    pub version: u32,
    pub name: String,
    /// When the session was first saved.
    pub created_at: DateTime<Utc>,
    /// When it was downloaded; ignored on upload.
    #[serde(default)]
    pub exported_at: Option<DateTime<Utc>>,
    /// A planner session's plan as it stood when saved; absent for CaiYatra.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<Plan>,
    /// What the page showed, in the page's own shape: for the planner the values the plan was shown
    /// against, for CaiYatra every input and the portfolio it started from.
    #[schema(value_type = Object)]
    pub page: serde_json::Value,
}

/// Record a session saved on a page, stamped now.
pub async fn create(
    db: &DatabaseConnection,
    user_id: i32,
    kind: Kind,
    name: &str,
    plan: Option<&Plan>,
    page: &serde_json::Value,
) -> AppResult<plan_session::Model> {
    let opts = options(db, user_id, kind).await;
    if !opts.enabled {
        return Err(off(kind, "recording"));
    }
    let new = New { name: name.trim().to_string(), plan, page, created_at: Utc::now().naive_utc(), uploaded_at: None };
    let kept = list(db, user_id, kind).await?;
    if same_name(&kept, &new.name, None) {
        return Err(AppError::Conflict(format!(
            "a saved session is already named \"{}\" - choose another name",
            new.name
        )));
    }
    insert(db, user_id, kind, opts, &kept, new, None).await
}

fn off(kind: Kind, what: &str) -> AppError {
    AppError::forbidden(format!(
        "{what} {} sessions is turned off in your profile, under {}",
        kind.label(),
        kind.card()
    ))
}

/// The session in an uploaded document: one downloaded from the API, or the
/// page's own JSON download, which carries one under `session`.
pub fn session_of(kind: Kind, body: serde_json::Value) -> AppResult<SessionDoc> {
    let inner = if body.get("format").and_then(|f| f.as_str()) == Some(kind.page_format()) {
        body.get("session").cloned().ok_or_else(|| AppError::bad("this download carries no session"))?
    } else {
        body
    };
    // serde's own message for a bad timestamp names neither the field nor the form it wants.
    if let Some(at) = inner.get("created_at").filter(|v| !v.is_null()) {
        if !at.as_str().is_some_and(|s| s.parse::<DateTime<Utc>>().is_ok()) {
            return Err(AppError::bad(format!(
                "not a saved {} session: created_at must be a date and time with its offset, like 2026-10-18T11:59:54Z",
                kind.label()
            )));
        }
    }
    serde_json::from_value(inner)
        .map_err(|e| AppError::bad(format!("not a saved {} session: {e}", kind.label())))
}

/// Record a session from a downloaded file, keeping when it was first
/// saved. A session kept already from the same moment (to the second) is
/// replaced only with `overwrite`; otherwise the upload is refused with
/// `409`. A name taken by another session gets ` (2)`, ` (3)`, ...
pub async fn import(
    db: &DatabaseConnection,
    user_id: i32,
    kind: Kind,
    doc: &SessionDoc,
    overwrite: bool,
) -> AppResult<plan_session::Model> {
    let opts = options(db, user_id, kind).await;
    if !opts.enabled {
        return Err(off(kind, "recording"));
    }
    if !opts.upload {
        return Err(off(kind, "uploading"));
    }
    if doc.format != kind.doc_format() || doc.version != DOC_VERSION {
        return Err(AppError::bad(format!(
            "not a saved {} session: expected format `{}`, version {DOC_VERSION}",
            kind.label(),
            kind.doc_format()
        )));
    }
    if kind == Kind::DhanYatra && doc.plan.is_some() {
        return Err(AppError::bad("not a saved CaiYatra session: it has no plan - a planner session goes to the planner"));
    }
    let now = Utc::now();
    if doc.created_at > now {
        return Err(AppError::bad("created_at is in the future"));
    }
    let created_at = doc.created_at.naive_utc();
    let kept = list(db, user_id, kind).await?;
    let replacing = kept.iter().find(|s| s.created_at.trunc_subsecs(0) == created_at.trunc_subsecs(0)).map(|s| s.id);
    if let (Some(_), false) = (replacing, overwrite) {
        return Err(AppError::Conflict(format!(
            "a saved session from {} UTC is kept already - upload with overwrite to replace it",
            created_at.format("%Y-%m-%d %H:%M:%S")
        )));
    }
    let base = doc.name.trim().to_string();
    let mut name = base.clone();
    let mut n = 2;
    while same_name(&kept, &name, replacing) {
        name = numbered(&base, n);
        n += 1;
    }
    let new = New { name, plan: doc.plan.as_ref(), page: &doc.page, created_at, uploaded_at: Some(now.naive_utc()) };
    insert(db, user_id, kind, opts, &kept, new, replacing).await
}

/// `base (n)`, `base` shortened so the whole stays within [`MAX_NAME`]: a name
/// that fits, taken already, still fits once numbered.
fn numbered(base: &str, n: u32) -> String {
    let suffix = format!(" ({n})");
    let room = MAX_NAME.saturating_sub(suffix.chars().count());
    let stem: String = base.chars().take(room).collect();
    format!("{}{suffix}", stem.trim_end())
}

/// Another session than `except` already has this name, ignoring case.
fn same_name(kept: &[Listed], name: &str, except: Option<i32>) -> bool {
    kept.iter().any(|s| Some(s.id) != except && s.name.to_lowercase() == name.to_lowercase())
}

/// A session about to be recorded.
struct New<'a> {
    name: String,
    plan: Option<&'a Plan>,
    page: &'a serde_json::Value,
    created_at: NaiveDateTime,
    uploaded_at: Option<NaiveDateTime>,
}

/// Record `new`, in place of the session `replacing` if given - which keeps its id.
async fn insert(
    db: &DatabaseConnection,
    user_id: i32,
    kind: Kind,
    opts: Options,
    kept: &[Listed],
    new: New<'_>,
    replacing: Option<i32>,
) -> AppResult<plan_session::Model> {
    if new.name.is_empty() || new.name.chars().count() > MAX_NAME {
        return Err(AppError::bad(format!("give the session a name of 1 to {MAX_NAME} characters")));
    }
    let plan = match (kind, new.plan) {
        (Kind::Planner, Some(plan)) => {
            planner::validate(plan)?;
            serde_json::to_string(plan).map_err(|e| AppError::Other(e.into()))?
        }
        (Kind::Planner, None) => return Err(AppError::bad("a planner session needs its plan")),
        (Kind::DhanYatra, _) => "null".to_string(),
    };
    check_page(kind, new.page)?;
    let page = serde_json::to_string(new.page).map_err(|e| AppError::Other(e.into()))?;
    if page.len() > MAX_PAGE_BYTES {
        return Err(AppError::bad("this session is too big to keep"));
    }

    let others = kept.len() - usize::from(replacing.is_some());
    if others as u64 >= u64::from(opts.max) {
        return Err(AppError::Conflict(format!(
            "you keep {} saved {} sessions and allow {} - delete one or allow more in your profile, under {}",
            kept.len(),
            kind.label(),
            opts.max,
            kind.card()
        )));
    }

    let row = plan_session::ActiveModel {
        user_id: Set(user_id),
        name: Set(new.name),
        created_at: Set(new.created_at),
        plan: Set(plan),
        page: Set(page),
        uploaded_at: Set(new.uploaded_at),
        kind: Set(kind.key().to_string()),
        ..Default::default()
    };
    // `replacing` comes from this user's sessions of this page, so its id alone is enough.
    Ok(match replacing {
        Some(id) => plan_session::ActiveModel { id: Set(id), ..row }.update(db).await?,
        None => row.insert(db).await?,
    })
}

/// The page's values, as far as the server can tell: the shape the page reads back.
fn check_page(kind: Kind, page: &serde_json::Value) -> AppResult<()> {
    let versioned = page.get("version").and_then(|v| v.as_u64()) == Some(1);
    let has = |keys: &[&str]| keys.iter().all(|k| page.get(*k).is_some_and(|v| v.is_object()));
    let (ok, shape) = match kind {
        Kind::Planner => (versioned && has(&["data", "segSubs", "clsSubs"]), "`data`, `segSubs` and `clsSubs` objects"),
        Kind::DhanYatra => (versioned && has(&["inputs"]), "an `inputs` object"),
    };
    if ok {
        Ok(())
    } else {
        Err(AppError::bad(format!(
            "page must be the {} page's values: an object with `version` 1 and {shape}",
            kind.label()
        )))
    }
}

/// One of `user_id`'s sessions of a page. Someone else's is "not found".
pub async fn get(db: &DatabaseConnection, user_id: i32, kind: Kind, id: i32) -> AppResult<plan_session::Model> {
    plan_session::Entity::find_by_id(id)
        .filter(plan_session::Column::UserId.eq(user_id))
        .filter(plan_session::Column::Kind.eq(kind.key()))
        .one(db)
        .await?
        .ok_or_else(|| AppError::not_found("saved session", id))
}

/// A stored plan: a planner session's; `None` for CaiYatra.
pub fn plan_of(m: &plan_session::Model) -> AppResult<Option<Plan>> {
    serde_json::from_str(&m.plan).map_err(|e| AppError::Other(e.into()))
}

/// One of `user_id`'s sessions as a document to download, and a file name for it.
pub async fn export(db: &DatabaseConnection, user_id: i32, kind: Kind, id: i32) -> AppResult<(SessionDoc, String)> {
    if !options(db, user_id, kind).await.download {
        return Err(off(kind, "downloading"));
    }
    let m = get(db, user_id, kind, id).await?;
    let created_at = m.created_at.and_utc();
    let file = format!("{}_{}.json", file_safe(&m.name), created_at.format("%Y-%m-%d_%H-%M-%S"));
    let doc = SessionDoc {
        format: kind.doc_format().into(),
        version: DOC_VERSION,
        plan: plan_of(&m)?,
        name: m.name,
        created_at,
        exported_at: Some(Utc::now()),
        page: serde_json::from_str(&m.page).map_err(|e| AppError::Other(e.into()))?,
    };
    Ok((doc, file))
}

/// A name as it can go in a file name: letters, digits, `-` and `_`.
pub fn file_safe(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if s.is_empty() { "session".into() } else { s }
}

/// Delete one of `user_id`'s sessions of a page. Someone else's is "not found".
pub async fn delete(db: &DatabaseConnection, user_id: i32, kind: Kind, id: i32) -> AppResult<()> {
    // Out of every share it is in first: only once it is known to be this user's.
    if kind == Kind::Planner {
        get(db, user_id, kind, id).await?;
        super::plan_shares::forget_session(db, id).await?;
    }
    let gone = plan_session::Entity::delete_many()
        .filter(plan_session::Column::Id.eq(id))
        .filter(plan_session::Column::UserId.eq(user_id))
        .filter(plan_session::Column::Kind.eq(kind.key()))
        .exec(db)
        .await?
        .rows_affected;
    if gone == 0 {
        return Err(AppError::not_found("saved session", id));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_limit_is_a_whole_number_within_bounds() {
        assert_eq!(parse_max("10"), Some(10));
        assert_eq!(parse_max(" 1 "), Some(1));
        assert_eq!(parse_max("100"), Some(100));
        assert_eq!(parse_max("0"), None);
        assert_eq!(parse_max("101"), None);
        assert_eq!(parse_max("2.5"), None);
        assert_eq!(parse_max("ten"), None);
    }

    #[test]
    fn names_are_made_safe_for_files() {
        assert_eq!(file_safe("Before March: rebalance"), "Before_March__rebalance");
        assert_eq!(file_safe("plan-2_a"), "plan-2_a");
        assert_eq!(file_safe("  "), "session");
    }

    #[test]
    fn only_the_pages_own_shape_is_kept() {
        let good = serde_json::json!({"version": 1, "data": {}, "segSubs": {}, "clsSubs": {}});
        assert!(check_page(Kind::Planner, &good).is_ok());
        assert!(check_page(Kind::DhanYatra, &good).is_err());
        assert!(check_page(Kind::Planner, &serde_json::json!({"version": 1})).is_err());
        assert!(check_page(Kind::Planner, &serde_json::json!([])).is_err());
        let yatra = serde_json::json!({"version": 1, "inputs": {}});
        assert!(check_page(Kind::DhanYatra, &yatra).is_ok());
        assert!(check_page(Kind::DhanYatra, &serde_json::json!({"version": 2, "inputs": {}})).is_err());
    }

    #[test]
    fn a_download_names_the_session_inside() {
        let doc = serde_json::json!({"format": "cairezeki-dhan-yatra", "session": {
            "format": "cairezeki-dhan-yatra-session", "version": 1, "name": "n",
            "created_at": "2026-01-01T00:00:00Z", "page": {"version": 1, "inputs": {}}
        }});
        let s = session_of(Kind::DhanYatra, doc).unwrap();
        assert_eq!(s.format, "cairezeki-dhan-yatra-session");
        assert!(s.plan.is_none());
    }

    #[test]
    fn a_numbered_name_still_fits() {
        assert_eq!(numbered("Optimal", 2), "Optimal (2)");
        let long = "L".repeat(MAX_NAME);
        let n = numbered(&long, 2);
        assert_eq!(n.chars().count(), MAX_NAME);
        assert!(n.ends_with("L (2)"));
        assert_eq!(numbered(&format!("{} bc", "a".repeat(MAX_NAME - 5)), 2).chars().count(), MAX_NAME - 1, "no space left before the number");
    }

    #[test]
    fn a_bad_moment_is_named_as_such() {
        let doc = |at: serde_json::Value| serde_json::json!({
            "format": "cairezeki-dhan-yatra-session", "version": 1, "name": "n",
            "created_at": at, "page": {"version": 1, "inputs": {}}
        });
        for bad in [serde_json::json!("yesterday"), serde_json::json!(5), serde_json::json!("2026-10-18T11:59:54")] {
            let e = session_of(Kind::DhanYatra, doc(bad)).unwrap_err().to_string();
            assert!(e.contains("created_at"), "{e}");
        }
        assert!(session_of(Kind::DhanYatra, doc(serde_json::json!("2026-10-18T19:59:54+08:00"))).is_ok());
    }
}
