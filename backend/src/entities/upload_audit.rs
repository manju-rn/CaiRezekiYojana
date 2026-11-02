use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// One step in the upload audit: a file validated or imported, or an import
/// undone. Rows are only ever added - `id` is the audit sequence number.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = UploadAudit)]
#[sea_orm(table_name = "upload_audit")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// The user these records belong to.
    pub user_id: i32,
    pub created_at: DateTime,
    /// `VALIDATE`, `IMPORT` or `UNDO`.
    pub action: String,
    /// `crate::types::ImportKind`.
    pub kind: String,
    /// The name the file was uploaded under.
    pub original_name: String,
    /// The file's name in the uploads folder; `None` when no folder is
    /// configured. For an undo, the file of the import it reverted.
    pub stored_as: Option<String>,
    pub size_bytes: i64,
    /// Hex SHA-256 of the file as uploaded, so the kept copy can be checked.
    pub sha256: Option<String>,
    /// The import's `ImportStatus`, `FAILED` when it was refused outright,
    /// or `REVERTED` for an undo.
    pub status: String,
    pub batch_id: Option<i32>,
    pub row_count: i32,
    pub ok_count: i32,
    pub skipped_count: i32,
    pub error_count: i32,
    pub message: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
