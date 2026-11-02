//! A saved planner session can be downloaded as JSON and uploaded again.
//! An uploaded one keeps the moment it was first saved in `created_at`;
//! `uploaded_at` records when it came back in. `NULL`: saved here.

use sea_orm_migration::prelude::*;

use super::idens::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.alter_table(
            Table::alter()
                .table(PlanSession::Table)
                .add_column(ColumnDef::new(PlanSession::UploadedAt).date_time().null())
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.alter_table(Table::alter().table(PlanSession::Table).drop_column(PlanSession::UploadedAt).to_owned()).await
    }
}
