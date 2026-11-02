use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A point-in-time balance for a bank/savings account. Balances are snapshots,
/// not a ledger: the latest row per account is the current balance.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = CashBalance)]
#[sea_orm(table_name = "cash_balance")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub account_id: i32,
    pub as_of: Date,
    pub balance: f64,
    pub currency_code: String,
    /// `manual`, `import`.
    pub source: String,
    pub notes: Option<String>,
    pub created_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
