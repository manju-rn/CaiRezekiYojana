use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A session recorded by its owner on the planner (the plan as it stood and
/// the values it was shown against) or on CaiYatra (every input and the
/// portfolio it started from), as JSON. Never changed once saved; deleted
/// only by its owner (or with the account).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "plan_session")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub user_id: i32,
    pub name: String,
    pub created_at: DateTime,
    /// A planner session's plan, as `services::planner::Plan` writes it; `null` for CaiYatra.
    #[sea_orm(column_type = "Text")]
    pub plan: String,
    /// What the page needs to show the session as it was when saved. The
    /// page's own shape; the server only checks it is that shape.
    #[sea_orm(column_type = "Text")]
    pub page: String,
    /// When it was uploaded from a downloaded file; `None`: saved here.
    pub uploaded_at: Option<DateTime>,
    /// Whose it is: `planner` or `dhan_yatra`.
    pub kind: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
