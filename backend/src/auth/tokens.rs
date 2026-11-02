//! Random tokens, and the single-use ones that activate an account, reset
//! its password or confirm its email address.
//!
//! Only a token's SHA-256 is stored. The token itself is shown once - to
//! the administrator who issued it, or in the email that carries it - so
//! the database alone cannot be used to redeem one.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{Duration, NaiveDateTime, Utc};
use rand::RngCore;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
};
use sha2::{Digest, Sha256};

use crate::entities::user_token;
use crate::error::{AppError, AppResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// An account an administrator created: redeeming sets its password.
    Activate,
    /// A forgotten password: redeeming sets a new one.
    Reset,
    /// Confirms the email address an account signed up with.
    Verify,
}

impl Purpose {
    pub fn as_str(&self) -> &'static str {
        match self {
            Purpose::Activate => "ACTIVATE",
            Purpose::Reset => "RESET",
            Purpose::Verify => "VERIFY",
        }
    }

    /// How long a token stays good.
    pub fn lifetime(&self) -> Duration {
        match self {
            Purpose::Activate => Duration::days(7),
            Purpose::Reset => Duration::hours(24),
            Purpose::Verify => Duration::days(3),
        }
    }
}

/// `bytes` of randomness from the OS, URL-safe base64.
pub fn random(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rngs::OsRng.fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

pub fn hash(token: &str) -> String {
    Sha256::digest(token.trim().as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Compare without leaking a per-byte timing signal.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Issue a token, replacing any unredeemed one for the same purpose, and
/// return it with when it expires.
pub async fn issue(db: &DatabaseConnection, user_id: i32, purpose: Purpose) -> AppResult<(String, NaiveDateTime)> {
    revoke(db, user_id, purpose).await?;
    let token = random(24);
    let now = Utc::now().naive_utc();
    let expires_at = now + purpose.lifetime();
    user_token::ActiveModel {
        user_id: Set(user_id),
        purpose: Set(purpose.as_str().to_string()),
        token_hash: Set(hash(&token)),
        expires_at: Set(expires_at),
        used_at: Set(None),
        created_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok((token, expires_at))
}

/// Unredeemed tokens for `purpose` stop working.
pub async fn revoke(db: &DatabaseConnection, user_id: i32, purpose: Purpose) -> AppResult<()> {
    user_token::Entity::delete_many()
        .filter(user_token::Column::UserId.eq(user_id))
        .filter(user_token::Column::Purpose.eq(purpose.as_str()))
        .filter(user_token::Column::UsedAt.is_null())
        .exec(db)
        .await?;
    Ok(())
}

/// The user a good token was issued to. Does not use it up - call
/// [`consume`] once whatever it was for has succeeded.
pub async fn check(db: &DatabaseConnection, token: &str, purposes: &[Purpose]) -> AppResult<user_token::Model> {
    let bad = || AppError::bad("That token is not valid - it may have expired or been used already.");
    let row = user_token::Entity::find()
        .filter(user_token::Column::TokenHash.eq(hash(token)))
        .one(db)
        .await?
        .ok_or_else(bad)?;
    let fits = purposes.iter().any(|p| p.as_str() == row.purpose);
    if !fits || row.used_at.is_some() || row.expires_at <= Utc::now().naive_utc() {
        return Err(bad());
    }
    Ok(row)
}

pub async fn consume(db: &DatabaseConnection, row: user_token::Model) -> AppResult<()> {
    let mut m: user_token::ActiveModel = row.into();
    m.used_at = Set(Some(Utc::now().naive_utc()));
    m.update(db).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_and_url_safe() {
        let a = random(24);
        let b = random(24);
        assert_ne!(a, b);
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn hashing_ignores_stray_whitespace_from_a_paste() {
        assert_eq!(hash("abc"), hash(" abc\n"));
        assert_eq!(hash("abc").len(), 64);
    }

    #[test]
    fn comparison_is_exact() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secret-longer"));
        assert!(!constant_time_eq(b"", b"x"));
    }
}
