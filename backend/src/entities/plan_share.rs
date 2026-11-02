use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A user's saved planner sessions shared with another user (`kind`
/// `user`, `recipient_id`) or as a link (`kind` `link`, `token`), read-only.
/// Which sessions it shows is `plan_share_session`. Revoked by deleting it.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "plan_share")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub owner_id: i32,
    /// `user` or `link`.
    pub kind: String,
    /// Whom a `user` share is with; `None` for a link.
    pub recipient_id: Option<i32>,
    /// A link's secret, the last part of its URL; `None` for a user share.
    /// Kept as it is, so the owner can copy the link again.
    pub token: Option<String>,
    /// The owner's own name for it; may be empty.
    pub label: String,
    pub created_at: DateTime,
    /// After this it opens for nobody; `None`: until revoked.
    pub expires_at: Option<DateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
