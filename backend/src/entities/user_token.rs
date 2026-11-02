use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A single-use token: to activate an account an administrator created, to
/// set a new password, or to confirm an email address. Only its SHA-256 is
/// kept; the token itself is shown once, to whoever issued it.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "user_token")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub user_id: i32,
    /// `ACTIVATE`, `RESET` or `VERIFY`.
    pub purpose: String,
    pub token_hash: String,
    pub expires_at: DateTime,
    pub used_at: Option<DateTime>,
    pub created_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
