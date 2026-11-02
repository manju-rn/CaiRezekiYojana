use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// ISO-4217 reference data. Seeded by the migration, extendable via the API.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = Currency)]
#[sea_orm(table_name = "currency")]
pub struct Model {
    /// ISO 4217 alpha-3, e.g. `INR`, `SGD`, `AUD`, `USD`.
    #[sea_orm(primary_key, auto_increment = false)]
    pub code: String,
    pub name: String,
    pub symbol: String,
    /// Digits after the decimal point for display (2 for INR/USD, 0 for JPY).
    pub minor_units: i32,
    pub is_active: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
