use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// An OAuth login linked to a user: the provider's id for them, which -
/// unlike an email address - never changes.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "user_identity")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub user_id: i32,
    /// `google`, `github` or `oidc`.
    pub provider: String,
    pub subject: String,
    pub email: Option<String>,
    pub created_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
