//! Password hashing: Argon2id with a random salt, stored as a PHC string.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;

use crate::error::{AppError, AppResult};

pub const MIN_LENGTH: usize = 8;

/// Long enough, not too long to hash, and not just the email again.
pub fn validate(password: &str, email: &str) -> AppResult<()> {
    let chars = password.chars().count();
    if chars < MIN_LENGTH {
        return Err(AppError::bad(format!("the password must be at least {MIN_LENGTH} characters")));
    }
    if chars > 256 {
        return Err(AppError::bad("the password must be at most 256 characters"));
    }
    if password.trim().eq_ignore_ascii_case(email.trim()) {
        return Err(AppError::bad("the password cannot be your email address"));
    }
    Ok(())
}

/// Hashing is deliberately slow, so it runs off the async threads.
pub async fn hash(password: &str) -> AppResult<String> {
    let password = password.to_string();
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| AppError::Other(anyhow::anyhow!("could not hash the password: {e}")))
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!(e)))?
}

pub async fn verify(password: &str, stored: &str) -> bool {
    let (password, stored) = (password.to_string(), stored.to_string());
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&stored)
            .map(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_hash_verifies_only_its_own_password() {
        let h = hash("correct horse").await.unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify("correct horse", &h).await);
        assert!(!verify("correct horsE", &h).await);
        assert!(!verify("correct horse", "not a hash").await);
    }

    #[test]
    fn weak_passwords_are_refused() {
        assert!(validate("short", "a@b.io").is_err());
        assert!(validate("a@b.io", "A@B.io").is_err());
        assert!(validate("long enough", "a@b.io").is_ok());
    }
}
