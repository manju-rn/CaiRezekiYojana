//! AI mode. `ai_service_key` holds the keys an administrator mints for n8n:
//! they identify n8n to CaiRezekiYojana and nothing more. `ai_request` is one
//! question from the chat: an unguessable id, short-lived and revocable,
//! that stands for the user behind it on the AI endpoints. For both only
//! the SHA-256 is kept.

use sea_orm_migration::prelude::*;

use super::idens::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.create_table(
            Table::create()
                .table(AiServiceKey::Table)
                .if_not_exists()
                .col(ColumnDef::new(AiServiceKey::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(AiServiceKey::Name).string_len(120).not_null())
                .col(ColumnDef::new(AiServiceKey::Prefix).string_len(16).not_null())
                .col(ColumnDef::new(AiServiceKey::TokenHash).string_len(64).not_null().unique_key())
                .col(ColumnDef::new(AiServiceKey::CreatedBy).integer().null())
                .col(ColumnDef::new(AiServiceKey::CreatedAt).date_time().not_null())
                .col(ColumnDef::new(AiServiceKey::RotatedAt).date_time().null())
                .col(ColumnDef::new(AiServiceKey::LastUsedAt).date_time().null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_ai_service_key_created_by")
                        .from(AiServiceKey::Table, AiServiceKey::CreatedBy)
                        .to(AppUser::Table, AppUser::Id)
                        .on_delete(ForeignKeyAction::SetNull),
                )
                .to_owned(),
        )
        .await?;

        m.create_table(
            Table::create()
                .table(AiRequest::Table)
                .if_not_exists()
                .col(ColumnDef::new(AiRequest::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(AiRequest::TokenHash).string_len(64).not_null().unique_key())
                .col(ColumnDef::new(AiRequest::UserId).integer().not_null())
                .col(ColumnDef::new(AiRequest::Role).string_len(16).not_null())
                .col(ColumnDef::new(AiRequest::Capabilities).string_len(255).not_null().default(""))
                .col(ColumnDef::new(AiRequest::Feature).string_len(32).null())
                .col(ColumnDef::new(AiRequest::CreatedAt).date_time().not_null())
                .col(ColumnDef::new(AiRequest::ExpiresAt).date_time().not_null())
                .col(ColumnDef::new(AiRequest::RevokedAt).date_time().null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_ai_request_user")
                        .from(AiRequest::Table, AiRequest::UserId)
                        .to(AppUser::Table, AppUser::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ix_ai_request_user")
                .table(AiRequest::Table)
                .col(AiRequest::UserId)
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ix_ai_request_expires")
                .table(AiRequest::Table)
                .col(AiRequest::ExpiresAt)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_table(Table::drop().table(AiRequest::Table).to_owned()).await?;
        m.drop_table(Table::drop().table(AiServiceKey::Table).to_owned()).await
    }
}
