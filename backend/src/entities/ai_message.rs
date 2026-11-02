use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// One message of an AI mode conversation: the user's question or the
/// assistant's answer.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "ai_message")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub conversation_id: i32,
    /// `user` or `assistant`.
    pub role: String,
    #[sea_orm(column_type = "Text")]
    pub content: String,
    /// The capability that answered; `None` for the user's own messages.
    pub feature: Option<String>,
    /// `done`, or `error` for an answer that failed - its content then says why.
    pub status: String,
    /// The page the user was on when they asked.
    pub page: Option<String>,
    pub created_at: DateTime,
    /// What this answer changed on one of the user's pages, JSON - `{page, inputs}`.
    #[sea_orm(column_type = "Text", nullable)]
    pub page_action: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
