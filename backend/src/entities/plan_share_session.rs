use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// One saved planner session in a share.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "plan_share_session")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub share_id: i32,
    pub session_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
