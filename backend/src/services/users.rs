//! Accounts: creating them, finding them, and removing one with
//! everything it owns.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait,
    QueryFilter, TransactionTrait,
};

use crate::auth::{normalise_email, password, Role};
use crate::entities::{account, ai_conversation, app_user, fx_setting, import_batch, upload_audit, user_setting};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

use super::prefs;

pub struct NewUser<'a> {
    pub email: &'a str,
    pub display_name: &'a str,
    pub role: Role,
    /// Plain text, hashed here. `None` for an account an administrator
    /// creates (activated by token) or one that signs in only via OAuth.
    pub password: Option<&'a str>,
    pub email_verified: bool,
    pub must_set_password: bool,
    /// Signed up by themselves: disabled until an administrator enables it.
    pub awaiting_approval: bool,
    /// A user's reporting currency; ignored for other roles.
    pub base_currency: Option<&'a str>,
}

pub async fn find_by_email(db: &DatabaseConnection, email: &str) -> AppResult<Option<app_user::Model>> {
    Ok(app_user::Entity::find()
        .filter(app_user::Column::Email.eq(email.trim().to_lowercase()))
        .one(db)
        .await?)
}

/// The ids of `owner`'s accounts - what their transactions, deposits and
/// balances hang from.
pub async fn account_ids(db: &DatabaseConnection, owner: i32) -> AppResult<Vec<i32>> {
    Ok(account::Entity::find()
        .filter(account::Column::UserId.eq(owner))
        .all(db)
        .await?
        .into_iter()
        .map(|a| a.id)
        .collect())
}

/// Whether `account_id` is one of `owner`'s. Anything else is reported
/// as not found, so ids reveal nothing about other users.
pub async fn owned_account(db: &DatabaseConnection, owner: i32, account_id: i32) -> AppResult<account::Model> {
    account::Entity::find_by_id(account_id)
        .filter(account::Column::UserId.eq(owner))
        .one(db)
        .await?
        .ok_or_else(|| AppError::not_found("account", account_id))
}

pub async fn get(db: &DatabaseConnection, id: i32) -> AppResult<app_user::Model> {
    app_user::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| AppError::not_found("user", id))
}

pub async fn create(state: &AppState, new: NewUser<'_>) -> AppResult<app_user::Model> {
    let email = normalise_email(new.email)?;
    if find_by_email(&state.db, &email).await?.is_some() {
        return Err(AppError::Conflict(format!("an account for {email} already exists")));
    }
    let display_name = match new.display_name.trim() {
        "" => email.split('@').next().unwrap_or(&email).to_string(),
        name => name.chars().take(120).collect(),
    };
    let password_hash = match new.password {
        Some(p) => {
            password::validate(p, &email)?;
            Some(password::hash(p).await?)
        }
        None => None,
    };
    let now = Utc::now().naive_utc();
    let user = app_user::ActiveModel {
        email: Set(email),
        display_name: Set(display_name),
        role: Set(new.role.to_string()),
        password_hash: Set(password_hash),
        email_verified: Set(new.email_verified),
        is_enabled: Set(!new.awaiting_approval),
        locked_until: Set(None),
        lock_reason: Set(None),
        must_set_password: Set(new.must_set_password),
        awaiting_approval: Set(new.awaiting_approval),
        created_at: Set(now),
        updated_at: Set(now),
        last_login_at: Set(None),
        ..Default::default()
    }
    .insert(&state.db)
    .await?;

    if new.role == Role::User {
        let base = match new.base_currency.map(str::trim).filter(|c| !c.is_empty()) {
            Some(c) => c.to_uppercase(),
            None => state.setting("default_base_currency", "SGD").await.to_uppercase(),
        };
        prefs::validate_own("base_currency", &base, &[])?;
        prefs::set(&state.db, user.id, "base_currency", &base).await?;
    }
    tracing::info!(user = user.id, role = %user.role, "account created");
    Ok(user)
}

pub async fn set_password(db: &DatabaseConnection, user: app_user::Model, new_password: &str) -> AppResult<app_user::Model> {
    password::validate(new_password, &user.email)?;
    let mut m: app_user::ActiveModel = user.into();
    m.password_hash = Set(Some(password::hash(new_password).await?));
    m.must_set_password = Set(false);
    m.updated_at = Set(Utc::now().naive_utc());
    Ok(m.update(db).await?)
}

pub async fn admin_exists(db: &DatabaseConnection) -> AppResult<bool> {
    Ok(app_user::Entity::find()
        .filter(app_user::Column::Role.eq(Role::Admin.as_str()))
        .count(db)
        .await?
        > 0)
}

/// Enabled administrators other than `except`, so the last one cannot be
/// disabled, locked or removed.
pub async fn other_active_admins(db: &DatabaseConnection, except: i32) -> AppResult<u64> {
    Ok(app_user::Entity::find()
        .filter(app_user::Column::Role.eq(Role::Admin.as_str()))
        .filter(app_user::Column::IsEnabled.eq(true))
        .filter(app_user::Column::Id.ne(except))
        .count(db)
        .await?)
}

/// Remove a user and everything they own. Accounts take their
/// transactions, deposits and balances with them; the kept upload files
/// stay on disk, as nothing uploaded is ever deleted.
pub async fn delete_with_records(db: &DatabaseConnection, user_id: i32) -> AppResult<()> {
    let tx = db.begin().await?;
    account::Entity::delete_many().filter(account::Column::UserId.eq(user_id)).exec(&tx).await?;
    import_batch::Entity::delete_many().filter(import_batch::Column::UserId.eq(user_id)).exec(&tx).await?;
    upload_audit::Entity::delete_many().filter(upload_audit::Column::UserId.eq(user_id)).exec(&tx).await?;
    fx_setting::Entity::delete_many().filter(fx_setting::Column::UserId.eq(user_id)).exec(&tx).await?;
    user_setting::Entity::delete_many().filter(user_setting::Column::UserId.eq(user_id)).exec(&tx).await?;
    super::plan_shares::forget_user(&tx, user_id).await?;
    // AI mode conversations take their messages with them.
    ai_conversation::Entity::delete_many().filter(ai_conversation::Column::UserId.eq(user_id)).exec(&tx).await?;
    // Sessions, tokens and OAuth links go by foreign key.
    app_user::Entity::delete_by_id(user_id).exec(&tx).await?;
    tx.commit().await?;
    tracing::info!(user = user_id, "account and its records deleted");
    Ok(())
}

/// Failed sign-ins per email address: after [`MAX_FAILURES`] within the
/// window, further attempts are refused until it passes. In memory - a
/// restart forgets, which is fine for slowing down guessing.
#[derive(Default)]
pub struct LoginThrottle(Mutex<HashMap<String, (u32, Instant)>>);

const MAX_FAILURES: u32 = 5;
const WINDOW: Duration = Duration::from_secs(15 * 60);

impl LoginThrottle {
    pub fn check(&self, email: &str) -> AppResult<()> {
        let map = self.0.lock().expect("throttle lock");
        match map.get(email) {
            Some((n, since)) if *n >= MAX_FAILURES && since.elapsed() < WINDOW => {
                let mins = (WINDOW - since.elapsed()).as_secs() / 60 + 1;
                Err(AppError::unauthorized(format!(
                    "Too many failed attempts - try again in {mins} minute(s)."
                )))
            }
            _ => Ok(()),
        }
    }

    pub fn failed(&self, email: &str) {
        let mut map = self.0.lock().expect("throttle lock");
        let entry = map.entry(email.to_string()).or_insert((0, Instant::now()));
        if entry.1.elapsed() >= WINDOW {
            *entry = (0, Instant::now());
        }
        entry.0 += 1;
    }

    pub fn succeeded(&self, email: &str) {
        self.0.lock().expect("throttle lock").remove(email);
    }
}

/// Instruments are shared market data, but which ones a user holds is
/// theirs: these are the ids held only by other users, which `owner` is
/// not shown.
pub async fn others_only_instruments(db: &DatabaseConnection, owner: i32) -> AppResult<std::collections::HashSet<i32>> {
    use crate::entities::txn;
    let mine = account_ids(db, owner).await?;
    let mut others = std::collections::HashSet::new();
    let mut own = std::collections::HashSet::new();
    for t in txn::Entity::find().all(db).await? {
        if mine.contains(&t.account_id) {
            own.insert(t.instrument_id);
        } else {
            others.insert(t.instrument_id);
        }
    }
    Ok(others.difference(&own).copied().collect())
}

/// Changing an instrument - its codes, its kind, a hand-entered price -
/// changes every holder's figures, so a user may only do it to one no
/// other user holds.
pub async fn check_instrument_editable(db: &DatabaseConnection, owner: i32, instrument_id: i32) -> AppResult<()> {
    use crate::entities::txn;
    if others_only_instruments(db, owner).await?.contains(&instrument_id) {
        return Err(AppError::not_found("instrument", instrument_id));
    }
    let mine = account_ids(db, owner).await?;
    let held_by_others = txn::Entity::find()
        .filter(txn::Column::InstrumentId.eq(instrument_id))
        .filter(txn::Column::AccountId.is_not_in(mine))
        .count(db)
        .await?;
    if held_by_others > 0 {
        return Err(AppError::forbidden(
            "other users hold this instrument too, so it cannot be changed from one account",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_throttle_trips_after_repeated_failures_and_resets_on_success() {
        let t = LoginThrottle::default();
        for _ in 0..MAX_FAILURES {
            assert!(t.check("a@b.io").is_ok());
            t.failed("a@b.io");
        }
        assert!(t.check("a@b.io").is_err());
        assert!(t.check("other@b.io").is_ok(), "per address");
        t.succeeded("a@b.io");
        assert!(t.check("a@b.io").is_ok());
    }
}
