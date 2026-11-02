use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// One of a user's own preferences - base currency, cost-basis method, page
/// visibility - or an administrator's override of one (`override_*`).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = UserSetting)]
#[sea_orm(table_name = "user_setting")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub user_id: i32,
    pub key: String,
    pub value: String,
    pub updated_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
