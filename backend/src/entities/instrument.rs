use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A tradeable thing: an equity, an ETF, or a mutual-fund scheme.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = Instrument)]
#[sea_orm(table_name = "instrument")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// `crate::types::AssetKind`.
    pub kind: String,
    /// Ticker (`INFY`, `VOO`) or AMFI scheme code for funds.
    pub symbol: String,
    pub name: String,
    pub isin: Option<String>,
    pub exchange_id: Option<i32>,
    /// Currency this instrument is *priced* in. May differ from the account's.
    pub currency_code: String,
    pub country: Option<String>,
    /// Fully-qualified symbol for the quote provider, e.g. `INFY.NS`, `VOO`.
    pub quote_symbol: Option<String>,
    /// AMFI scheme code - the join key for Indian mutual-fund NAV.
    pub amfi_scheme_code: Option<String>,
    pub sector: Option<String>,
    /// How far back a history feed has already filled `price_point` for
    /// this instrument. `None` until the first backfill; cleared when the
    /// scheme code changes.
    pub history_from: Option<Date>,
    /// The portfolio feed's identifier for this fund, learned from a match
    /// confirmed by scheme code or ISIN and kept so the next refresh skips
    /// the search. Set it by hand to point the feed at a different scheme;
    /// clear it to search afresh.
    pub holdings_ref: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
