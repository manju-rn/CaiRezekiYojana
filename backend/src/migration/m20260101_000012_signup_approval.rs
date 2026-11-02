//! Accounts made through sign-up wait for an administrator.
//!
//! `awaiting_approval` marks an account someone created for themselves -
//! the sign-up form or a first OAuth sign-in. It starts disabled, and
//! enabling it clears the mark. Every existing account is already approved.

use sea_orm_migration::prelude::*;

use super::idens::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.alter_table(
            Table::alter()
                .table(AppUser::Table)
                .add_column(ColumnDef::new(AppUser::AwaitingApproval).boolean().not_null().default(false))
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.alter_table(Table::alter().table(AppUser::Table).drop_column(AppUser::AwaitingApproval).to_owned()).await
    }
}
