//! Who is asking: always the one built-in owner.
//!
//! CaiRezekiYojana has no sign-in. The plans, sessions and portfolio belong
//! to a single account that [`ensure_owner`] makes at startup, and every
//! handler that wants "the caller" gets that account's id. The data layer
//! underneath still scopes everything by an owner id - so adding a real
//! sign-in later is a change to this module and nothing else.

pub mod password;
pub mod tokens;

use std::fmt;
use std::str::FromStr;
use std::sync::atomic::{AtomicI32, Ordering};

use axum::async_trait;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::services::users;
use crate::state::AppState;

// The built-in account's address (the same one the first accounts migration gives the owner profile). Never mailed.
pub use crate::migration::m20260101_000009_users_and_profiles::OWNER_EMAIL;

/// The built-in account's id, once [`ensure_owner`] has found or made it.
static OWNER_ID: AtomicI32 = AtomicI32::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Role {
    Admin,
    User,
    Exporter,
}

impl Role {
    pub const ALL: &'static [Role] = &[Role::Admin, Role::User, Role::Exporter];

    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Admin => "ADMIN",
            Role::User => "USER",
            Role::Exporter => "EXPORTER",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_uppercase().as_str() {
            "ADMIN" => Ok(Role::Admin),
            "USER" => Ok(Role::User),
            "EXPORTER" => Ok(Role::Exporter),
            other => Err(format!("unknown role '{other}' - expected ADMIN, USER or EXPORTER")),
        }
    }
}

/// The caller, as the handlers that want more than an id see them.
#[derive(Clone, Debug)]
pub struct CurrentUser {
    pub id: i32,
    pub role: Role,
    pub email: String,
    pub display_name: String,
}

impl CurrentUser {
    fn owner() -> Self {
        CurrentUser {
            id: OWNER_ID.load(Ordering::Relaxed),
            role: Role::User,
            email: OWNER_EMAIL.to_string(),
            display_name: "Owner".to_string(),
        }
    }
}

/// Find the built-in account, or make it, and remember its id. Called once at
/// startup, after the migrations.
pub async fn ensure_owner(state: &AppState) -> AppResult<()> {
    let owner = match users::find_by_email(&state.db, OWNER_EMAIL).await? {
        Some(u) => u,
        None => {
            users::create(
                state,
                users::NewUser {
                    email: OWNER_EMAIL,
                    display_name: "Owner",
                    role: Role::User,
                    password: None,
                    email_verified: true,
                    must_set_password: false,
                    awaiting_approval: false,
                    base_currency: None,
                },
            )
            .await?
        }
    };
    OWNER_ID.store(owner.id, Ordering::Relaxed);
    tracing::info!(owner = owner.id, "the built-in owner account is ready");
    Ok(())
}

/// The caller, in full.
pub struct Me(pub CurrentUser);

#[async_trait]
impl FromRequestParts<AppState> for Me {
    type Rejection = AppError;
    async fn from_request_parts(_: &mut Parts, _: &AppState) -> Result<Self, Self::Rejection> {
        Ok(Me(CurrentUser::owner()))
    }
}

/// The caller's id: what every query is scoped to.
#[derive(Clone, Copy, Debug)]
pub struct Owner(pub i32);

#[async_trait]
impl FromRequestParts<AppState> for Owner {
    type Rejection = AppError;
    async fn from_request_parts(_: &mut Parts, _: &AppState) -> Result<Self, Self::Rejection> {
        Ok(Owner(OWNER_ID.load(Ordering::Relaxed)))
    }
}

/// May trigger a market-data refresh. Anyone may.
pub struct Refresher;

#[async_trait]
impl FromRequestParts<AppState> for Refresher {
    type Rejection = AppError;
    async fn from_request_parts(_: &mut Parts, _: &AppState) -> Result<Self, Self::Rejection> {
        Ok(Refresher)
    }
}

/// Lowercased and trimmed, and roughly the shape of an address.
pub fn normalise_email(raw: &str) -> Result<String, AppError> {
    let email = raw.trim().to_lowercase();
    let valid = email.len() <= 254
        && !email.contains(char::is_whitespace)
        && email
            .split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.'));
    if valid {
        Ok(email)
    } else {
        Err(AppError::bad("enter a valid email address"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails_are_normalised_and_checked() {
        assert_eq!(normalise_email("  Me@Example.COM ").unwrap(), "me@example.com");
        assert!(normalise_email("nope").is_err());
        assert!(normalise_email("a@b").is_err());
        assert!(normalise_email("a b@c.io").is_err());
        assert!(normalise_email("@c.io").is_err());
    }

    #[test]
    fn the_built_in_owner_has_a_valid_address() {
        assert_eq!(normalise_email(OWNER_EMAIL).unwrap(), OWNER_EMAIL);
    }

    #[test]
    fn roles_round_trip() {
        for r in Role::ALL {
            assert_eq!(Role::from_str(r.as_str()).unwrap(), *r);
        }
        assert!(Role::from_str("root").is_err());
    }
}
