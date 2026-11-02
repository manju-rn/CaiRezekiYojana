use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A closing price / NAV for one instrument on one day.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = PricePoint)]
#[sea_orm(table_name = "price_point")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub instrument_id: i32,
    pub as_of: Date,
    pub close: f64,
    pub currency_code: String,
    /// `amfi`, `yahoo`, `manual`, `import`.
    pub source: String,
    pub created_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
