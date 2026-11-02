//! The upload audit: one row per file validated or imported and per import
//! undone, in order. Rows are only ever added, so the sequence of what was
//! uploaded, and what came of it, survives any later undo or re-import.

use sea_orm_migration::prelude::*;

use super::idens::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.create_table(
            Table::create()
                .table(UploadAudit::Table)
                .if_not_exists()
                .col(ColumnDef::new(UploadAudit::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(UploadAudit::CreatedAt).date_time().not_null())
                .col(ColumnDef::new(UploadAudit::Action).string_len(16).not_null())
                .col(ColumnDef::new(UploadAudit::Kind).string_len(32).not_null())
                .col(ColumnDef::new(UploadAudit::OriginalName).string_len(255).not_null())
                .col(ColumnDef::new(UploadAudit::StoredAs).string_len(255).null())
                .col(ColumnDef::new(UploadAudit::SizeBytes).big_integer().not_null())
                .col(ColumnDef::new(UploadAudit::Sha256).string_len(64).null())
                .col(ColumnDef::new(UploadAudit::Status).string_len(24).not_null())
                .col(ColumnDef::new(UploadAudit::BatchId).integer().null())
                .col(ColumnDef::new(UploadAudit::RowCount).integer().not_null())
                .col(ColumnDef::new(UploadAudit::OkCount).integer().not_null())
                .col(ColumnDef::new(UploadAudit::SkippedCount).integer().not_null())
                .col(ColumnDef::new(UploadAudit::ErrorCount).integer().not_null())
                .col(ColumnDef::new(UploadAudit::Message).text().null())
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_table(Table::drop().table(UploadAudit::Table).if_exists().to_owned()).await
    }
}
