use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// One conversation in AI mode, its owner's. `bytes` is the size of its
/// text, kept up to date as messages are added, for the per-user cap.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "ai_conversation")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub user_id: i32,
    /// From the first question.
    pub title: String,
    /// The capability that answered last, so a follow-up goes to it again.
    pub last_feature: Option<String>,
    pub bytes: i64,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
