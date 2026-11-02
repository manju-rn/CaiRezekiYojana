use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// One movement of an instrument in or out of an account.
///
/// `quantity` and `price` are always expressed in the instrument's own terms,
/// in `currency_code`. `fx_rate` snapshots the rate to the portfolio base
/// currency *on the trade date* so historical cost never drifts when today's
/// FX moves.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = Txn)]
#[sea_orm(table_name = "txn")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub account_id: i32,
    pub instrument_id: i32,
    pub trade_date: Date,
    /// `crate::types::TxnKind`.
    pub kind: String,
    /// Units/shares. For `SPLIT` this is the ratio multiplier (2.0 = 1-for-2).
    pub quantity: f64,
    /// Per-unit price in `currency_code`. Zero for BONUS/SPLIT.
    pub price: f64,
    pub fees: f64,
    pub taxes: f64,
    /// Signed cash impact in `currency_code`: negative for BUY, positive for
    /// SELL and DIVIDEND. Derived on write so reports never re-derive it.
    pub net_amount: f64,
    pub currency_code: String,
    /// Units of base currency per one `currency_code`, on `trade_date`.
    /// `None` means "same as base" (rate 1.0).
    pub fx_rate: Option<f64>,
    /// Broker/registrar reference. Unique per account; used to dedupe imports.
    pub external_id: Option<String>,
    pub import_batch_id: Option<i32>,
    pub notes: Option<String>,
    pub created_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
