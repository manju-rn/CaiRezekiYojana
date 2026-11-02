//! Personal API keys: long-lived bearer credentials a user or administrator
//! creates for a script or another program. Only the SHA-256 is stored;
//! the key is shown once, to whoever created it.

use sea_orm_migration::prelude::*;

use super::idens::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.create_table(
            Table::create()
                .table(ApiKey::Table)
                .if_not_exists()
                .col(ColumnDef::new(ApiKey::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(ApiKey::UserId).integer().not_null())
                .col(ColumnDef::new(ApiKey::Name).string_len(120).not_null())
                .col(ColumnDef::new(ApiKey::Prefix).string_len(16).not_null())
                .col(ColumnDef::new(ApiKey::TokenHash).string_len(64).not_null().unique_key())
                .col(ColumnDef::new(ApiKey::CreatedAt).date_time().not_null())
                .col(ColumnDef::new(ApiKey::ExpiresAt).date_time().null())
                .col(ColumnDef::new(ApiKey::LastUsedAt).date_time().null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_api_key_user")
                        .from(ApiKey::Table, ApiKey::UserId)
                        .to(AppUser::Table, AppUser::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_table(Table::drop().table(ApiKey::Table).to_owned()).await
    }
}
