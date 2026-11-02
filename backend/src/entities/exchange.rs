use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = Exchange)]
#[sea_orm(table_name = "exchange")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Short code used in the UI and in import files: `NSE`, `NASDAQ`, `ASX`.
    #[sea_orm(unique)]
    pub code: String,
    pub name: String,
    pub country: String,
    /// The currency instruments on this exchange trade in.
    pub currency_code: String,
    /// ISO 10383 Market Identifier Code, e.g. `XNSE`.
    pub mic: Option<String>,
    /// Suffix the quote provider appends to a ticker, e.g. `.NS`, `.AX`, `.SI`.
    pub quote_suffix: Option<String>,
    pub timezone: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
