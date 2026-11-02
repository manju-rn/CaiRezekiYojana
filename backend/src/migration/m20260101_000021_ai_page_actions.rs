//! AI mode answers that change what one of the user's pages shows - today,
//! the CaiYatra journey. The page keeps its inputs in the browser, so the
//! answer carries them there: noted on the AI request while it is answered,
//! then kept with the answer.

use sea_orm_migration::prelude::*;

use super::idens::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.alter_table(
            Table::alter()
                .table(AiRequest::Table)
                .add_column(ColumnDef::new(AiRequest::PageAction).text().null())
                .to_owned(),
        )
        .await?;
        m.alter_table(
            Table::alter()
                .table(AiMessage::Table)
                .add_column(ColumnDef::new(AiMessage::PageAction).text().null())
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.alter_table(Table::alter().table(AiMessage::Table).drop_column(AiMessage::PageAction).to_owned()).await?;
        m.alter_table(Table::alter().table(AiRequest::Table).drop_column(AiRequest::PageAction).to_owned()).await
    }
}
