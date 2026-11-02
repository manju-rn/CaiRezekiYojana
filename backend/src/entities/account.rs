use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A place where value is held: a demat account, an MF folio, a foreign
/// brokerage, a bank account, or a deposit book at one institution.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = Account)]
#[sea_orm(table_name = "account")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// The user these records belong to.
    pub user_id: i32,
    pub name: String,
    /// `crate::types::AccountKind`.
    pub kind: String,
    /// ISO 3166-1 alpha-2: `IN`, `SG`, `AU`, `US`.
    pub country: String,
    /// Currency the account itself is denominated in.
    pub currency_code: String,
    pub institution: Option<String>,
    /// Masked account/folio/DP identifier - never store the full number.
    pub account_ref: Option<String>,
    pub opened_on: Option<Date>,
    pub is_active: bool,
    pub notes: Option<String>,
    pub created_at: DateTime,
    pub updated_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
