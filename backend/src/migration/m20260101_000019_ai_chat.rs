//! AI mode's conversations: what each user asked and what the assistant
//! answered, kept by CaiRezekiYojana - n8n keeps nothing. `bytes` is a running
//! total of a conversation's text, for the per-user cap: over it, the oldest
//! conversations go first.

use sea_orm_migration::prelude::*;

use super::idens::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.create_table(
            Table::create()
                .table(AiConversation::Table)
                .if_not_exists()
                .col(ColumnDef::new(AiConversation::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(AiConversation::UserId).integer().not_null())
                .col(ColumnDef::new(AiConversation::Title).string_len(120).not_null())
                .col(ColumnDef::new(AiConversation::LastFeature).string_len(32).null())
                .col(ColumnDef::new(AiConversation::Bytes).big_integer().not_null().default(0))
                .col(ColumnDef::new(AiConversation::CreatedAt).date_time().not_null())
                .col(ColumnDef::new(AiConversation::UpdatedAt).date_time().not_null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_ai_conversation_user")
                        .from(AiConversation::Table, AiConversation::UserId)
                        .to(AppUser::Table, AppUser::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ix_ai_conversation_user")
                .table(AiConversation::Table)
                .col(AiConversation::UserId)
                .col(AiConversation::UpdatedAt)
                .to_owned(),
        )
        .await?;

        m.create_table(
            Table::create()
                .table(AiMessage::Table)
                .if_not_exists()
                .col(ColumnDef::new(AiMessage::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(AiMessage::ConversationId).integer().not_null())
                .col(ColumnDef::new(AiMessage::Role).string_len(16).not_null())
                .col(ColumnDef::new(AiMessage::Content).text().not_null())
                .col(ColumnDef::new(AiMessage::Feature).string_len(32).null())
                .col(ColumnDef::new(AiMessage::Status).string_len(16).not_null().default("done"))
                .col(ColumnDef::new(AiMessage::Page).string_len(64).null())
                .col(ColumnDef::new(AiMessage::CreatedAt).date_time().not_null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_ai_message_conversation")
                        .from(AiMessage::Table, AiMessage::ConversationId)
                        .to(AiConversation::Table, AiConversation::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ix_ai_message_conversation")
                .table(AiMessage::Table)
                .col(AiMessage::ConversationId)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_table(Table::drop().table(AiMessage::Table).to_owned()).await?;
        m.drop_table(Table::drop().table(AiConversation::Table).to_owned()).await
    }
}
