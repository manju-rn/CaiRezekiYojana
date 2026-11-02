use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A personal API key, for a script or another program to act as its
/// owner. Only its SHA-256 is kept; `prefix` is the first few characters,
/// so the owner can tell their keys apart.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "api_key")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub user_id: i32,
    pub name: String,
    pub prefix: String,
    pub token_hash: String,
    pub created_at: DateTime,
    pub expires_at: Option<DateTime>,
    pub last_used_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
