use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// One spreadsheet upload. Rows link back via `txn.import_batch_id` so a bad
/// import can be identified and rolled back.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = ImportBatch)]
#[sea_orm(table_name = "import_batch")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// The user these records belong to.
    pub user_id: i32,
    /// `crate::types::ImportKind`.
    pub kind: String,
    pub filename: String,
    /// `crate::types::ImportStatus`.
    pub status: String,
    pub row_count: i32,
    pub ok_count: i32,
    pub error_count: i32,
    pub skipped_count: i32,
    /// JSON array of `{ "row": n, "message": "..." }` for rejected rows.
    #[sea_orm(column_type = "Text", nullable)]
    pub errors_json: Option<String>,
    pub message: Option<String>,
    pub created_at: DateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
