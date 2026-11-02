use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Someone who can sign in. `role` is `crate::auth::Role`: an administrator
/// manages accounts and app-wide settings but has no portfolio; a user has
/// one; an exporter can read every user's records, only to export them.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "app_user")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Lowercased. What a user signs in with.
    pub email: String,
    pub display_name: String,
    pub role: String,
    /// Argon2id PHC string. `None` until the user sets one - an account an
    /// administrator created, or one that only ever signs in through OAuth.
    #[serde(skip_serializing)]
    pub password_hash: Option<String>,
    pub email_verified: bool,
    pub is_enabled: bool,
    /// Signing in is refused until then.
    pub locked_until: Option<DateTime>,
    pub lock_reason: Option<String>,
    /// Created by an administrator and not yet activated by its owner.
    pub must_set_password: bool,
    /// Signed up by themselves and not yet enabled by an administrator.
    /// Enabling the account clears it.
    pub awaiting_approval: bool,
    pub created_at: DateTime,
    pub updated_at: DateTime,
    pub last_login_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
