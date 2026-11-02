use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Per-pair policy: which provider feeds it, what margin to apply, and whether
/// a hand-entered rate overrides everything.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = FxSetting)]
#[sea_orm(table_name = "fx_setting")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// The user these records belong to.
    pub user_id: i32,
    pub base_code: String,
    pub quote_code: String,
    /// Provider id, or `default` to follow the app-wide provider setting.
    pub provider: String,
    /// `crate::types::MarginMode`.
    pub margin_mode: String,
    /// Percent (e.g. `-0.5`) or absolute quote-currency units, per margin_mode.
    /// Negative shades the rate down, positive shades it up.
    pub margin_value: f64,
    /// Hand-entered rate, used verbatim when `use_manual` is set.
    pub manual_rate: Option<f64>,
    pub use_manual: bool,
    pub note: Option<String>,
    pub updated_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
