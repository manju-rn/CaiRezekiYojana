use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A signed-in browser. The cookie carries a random token; only its SHA-256
/// is stored, so a copy of the database signs no one in.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "user_session")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub user_id: i32,
    pub token_hash: String,
    pub created_at: DateTime,
    pub expires_at: DateTime,
    pub last_seen_at: DateTime,
    pub user_agent: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
