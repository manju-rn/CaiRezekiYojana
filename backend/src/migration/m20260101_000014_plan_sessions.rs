//! Recorded RezekiYojana sessions: a plan kept as it stood when saved,
//! with the values the page showed it against, under a name the user gives
//! it. Kept until the user deletes it; how many a user may keep is their
//! `plan_sessions_max` setting.

use sea_orm_migration::prelude::*;

use super::idens::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.create_table(
            Table::create()
                .table(PlanSession::Table)
                .if_not_exists()
                .col(ColumnDef::new(PlanSession::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(PlanSession::UserId).integer().not_null())
                .col(ColumnDef::new(PlanSession::Name).string_len(120).not_null())
                .col(ColumnDef::new(PlanSession::CreatedAt).date_time().not_null())
                .col(ColumnDef::new(PlanSession::Plan).text().not_null())
                .col(ColumnDef::new(PlanSession::Page).text().not_null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_plan_session_user")
                        .from(PlanSession::Table, PlanSession::UserId)
                        .to(AppUser::Table, AppUser::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ix_plan_session_user")
                .table(PlanSession::Table)
                .col(PlanSession::UserId)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_table(Table::drop().table(PlanSession::Table).to_owned()).await
    }
}
