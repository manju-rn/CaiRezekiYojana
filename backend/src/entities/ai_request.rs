use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// One question asked in AI mode. Its id - shown once, only the SHA-256 is
/// kept - stands for the user on the AI endpoints until it expires or is
/// revoked, limited to the capabilities it was opened with.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "ai_request")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub token_hash: String,
    pub user_id: i32,
    /// The user's role when it was opened: `USER` or `ADMIN`.
    pub role: String,
    /// Comma-separated capability ids, e.g. `portfolio_qa,planner`.
    pub capabilities: String,
    /// The feature the question went to, once known.
    pub feature: Option<String>,
    pub created_at: DateTime,
    pub expires_at: DateTime,
    pub revoked_at: Option<DateTime>,
    /// What the answer changes on one of the user's pages, JSON - `{page, inputs?, rope?}` - noted while
    /// it is answered (`show_dhan_yatra`, `set_dhan_yatra_rope`).
    #[sea_orm(column_type = "Text", nullable)]
    pub page_action: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
