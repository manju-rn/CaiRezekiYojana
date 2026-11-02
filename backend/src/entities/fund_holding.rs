use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// One line of a mutual fund's disclosed portfolio: a security the fund
/// holds and the share of the fund's assets it accounts for.
///
/// Rows are a snapshot of the latest disclosure, replaced wholesale on each
/// refresh; `as_of` is the month-end the AMC published the portfolio for
/// and `created_at` is when it was fetched.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = FundHolding)]
#[sea_orm(table_name = "fund_holding")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub instrument_id: i32,
    /// The disclosure date.
    pub as_of: Date,
    pub name: String,
    /// `EQUITY`, `DEBT`, `CASH`, `MF` or `OTHER`, as the feed classifies it.
    pub nature: String,
    /// Finer type: `Equity`, `Certificate of Deposit`, `Repo`, ...
    pub instrument_type: Option<String>,
    pub sector: Option<String>,
    /// Share of the fund's assets, 0-100. Net payables can go negative.
    pub weight_pct: f64,
    /// What the fund itself holds of the security, in INR - not the user's
    /// share of it.
    pub market_value: Option<f64>,
    /// The feed's identifier for the security, when it has one: what lets
    /// the same stock be recognised across funds.
    pub security_ref: Option<String>,
    /// `groww`.
    pub source: String,
    pub created_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
