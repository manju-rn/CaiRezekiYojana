use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// A raw, un-adjusted rate as fetched from a provider.
///
/// Read as: 1 `base_code` = `rate` × `quote_code`. Margins and manual
/// overrides live in `fx_setting` and are applied on read, which keeps the
/// provider history pristine and auditable.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = FxRate)]
#[sea_orm(table_name = "fx_rate")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub base_code: String,
    pub quote_code: String,
    pub rate: f64,
    pub as_of: Date,
    /// `xe`, `frankfurter`, `exchangerate_host`, `manual`.
    pub source: String,
    pub created_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
