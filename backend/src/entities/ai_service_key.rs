use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A key an administrator mints for n8n, to call the AI endpoints. It
/// identifies n8n and nothing more: on its own it reaches no one's records.
/// Only its SHA-256 is kept; `prefix` tells keys apart.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "ai_service_key")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub prefix: String,
    pub token_hash: String,
    /// The administrator who minted it; `None` once their account is gone.
    pub created_by: Option<i32>,
    pub created_at: DateTime,
    /// When the secret was last replaced.
    pub rotated_at: Option<DateTime>,
    pub last_used_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
