use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// What a transfer service charges to convert one pair, sampled at several
/// amounts. Replaced on each fetch - a fee table is a snapshot, not a series.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = FxFee)]
#[sea_orm(table_name = "fx_fee")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// The currency the fee is charged in: the one being converted from.
    pub base_code: String,
    pub quote_code: String,
    /// `wise`.
    pub provider: String,
    /// How the money moves, e.g. `BALANCE>BALANCE` for a conversion inside
    /// Wise balances. Each route is priced differently.
    pub route: String,
    /// JSON `[[amount, fee], ...]`, both in `base_code`, ascending by amount.
    pub points: String,
    /// The provider's own mid-market rate when the fees were sampled.
    pub mid_rate: Option<f64>,
    pub fetched_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
