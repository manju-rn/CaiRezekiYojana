use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Changes an assistant proposed in AI mode, waiting for the user - or
/// approved, cancelled, expired or failed. All its changes go together.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "ai_proposal")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub user_id: i32,
    /// The AI request it was made in (`ai_request.id`).
    pub request_id: Option<i32>,
    /// The conversation and the answer it belongs to, once the answer is kept.
    pub conversation_id: Option<i32>,
    pub message_id: Option<i32>,
    /// What it changes: `plan`.
    pub kind: String,
    /// One line, from the assistant: "Move ₹5 lakh out of KOTAK BANK".
    pub title: String,
    /// The changes as proposed, JSON - applied again from these on approval.
    #[sea_orm(column_type = "Text")]
    pub changes: String,
    /// The changes in words, JSON, as they were checked when proposed.
    #[sea_orm(column_type = "Text")]
    pub lines: String,
    /// `pending`, `approved`, `cancelled`, `expired` or `failed`.
    pub status: String,
    /// Why it failed, when approving found the changes no longer fit.
    #[sea_orm(column_type = "Text", nullable)]
    pub outcome: Option<String>,
    pub created_at: DateTime,
    pub expires_at: DateTime,
    pub decided_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
