use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A term deposit: Indian FD/RD, Singapore fixed deposit, Australian term
/// deposit. Accrued value is computed, not stored, so it is always current.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = Deposit)]
#[sea_orm(table_name = "deposit")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub account_id: i32,
    /// `crate::types::DepositKind`.
    pub kind: String,
    pub institution: String,
    /// Masked deposit/receipt number.
    pub reference: Option<String>,
    /// Lump sum for FD; total contributed so far is derived for RD.
    pub principal: f64,
    /// Annual nominal rate as a percentage, e.g. `7.25` for 7.25% p.a.
    pub interest_rate: f64,
    /// `crate::types::Compounding`.
    pub compounding: String,
    /// `crate::types::PayoutFrequency`.
    pub payout_frequency: String,
    /// Per-period contribution for recurring deposits; `None` for FDs.
    pub installment: Option<f64>,
    pub start_date: Date,
    pub maturity_date: Date,
    pub currency_code: String,
    pub country: String,
    pub auto_renew: bool,
    /// `crate::types::DepositStatus`.
    pub status: String,
    /// Bank-quoted maturity value. When present it wins over our own maths,
    /// because banks round in their own particular ways.
    pub maturity_amount: Option<f64>,
    /// Tax withheld at source on the interest, as a percentage - `31.2` for
    /// an Indian NRO deposit. Taken out of the deposit at each 31 March and
    /// at maturity. `None` when nothing is withheld.
    pub tds_pct: Option<f64>,
    pub notes: Option<String>,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
