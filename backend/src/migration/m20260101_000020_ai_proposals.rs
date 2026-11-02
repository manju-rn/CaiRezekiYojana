//! AI mode's proposals: changes an assistant suggests and the user approves
//! or cancels in the chat. One proposal holds all the changes a request
//! needs, applied all or none. Kept apart from the conversations: they do
//! not count against the history cap and are not dropped with it.

use sea_orm_migration::prelude::*;

use super::idens::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.create_table(
            Table::create()
                .table(AiProposal::Table)
                .if_not_exists()
                .col(ColumnDef::new(AiProposal::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(AiProposal::UserId).integer().not_null())
                .col(ColumnDef::new(AiProposal::RequestId).integer().null())
                .col(ColumnDef::new(AiProposal::ConversationId).integer().null())
                .col(ColumnDef::new(AiProposal::MessageId).integer().null())
                .col(ColumnDef::new(AiProposal::Kind).string_len(32).not_null())
                .col(ColumnDef::new(AiProposal::Title).string_len(200).not_null())
                .col(ColumnDef::new(AiProposal::Changes).text().not_null())
                .col(ColumnDef::new(AiProposal::Lines).text().not_null())
                .col(ColumnDef::new(AiProposal::Status).string_len(16).not_null().default("pending"))
                .col(ColumnDef::new(AiProposal::Outcome).text().null())
                .col(ColumnDef::new(AiProposal::CreatedAt).date_time().not_null())
                .col(ColumnDef::new(AiProposal::ExpiresAt).date_time().not_null())
                .col(ColumnDef::new(AiProposal::DecidedAt).date_time().null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_ai_proposal_user")
                        .from(AiProposal::Table, AiProposal::UserId)
                        .to(AppUser::Table, AppUser::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ix_ai_proposal_user")
                .table(AiProposal::Table)
                .col(AiProposal::UserId)
                .col(AiProposal::Status)
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ix_ai_proposal_conversation")
                .table(AiProposal::Table)
                .col(AiProposal::ConversationId)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_table(Table::drop().table(AiProposal::Table).to_owned()).await
    }
}
