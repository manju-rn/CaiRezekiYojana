//! Shared planner sessions. A share is one row in `plan_share`: to another
//! user (`kind` `user`, `recipient_id`) or as a link anyone may open
//! (`kind` `link`, `token`). Which of the owner's saved planner sessions it
//! shows is `plan_share_session`, one row a session. Revoking a share
//! deletes it; deleting a session or either account takes it out of every
//! share.

use sea_orm_migration::prelude::*;

use super::idens::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.create_table(
            Table::create()
                .table(PlanShare::Table)
                .if_not_exists()
                .col(ColumnDef::new(PlanShare::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(PlanShare::OwnerId).integer().not_null())
                .col(ColumnDef::new(PlanShare::Kind).string_len(10).not_null())
                .col(ColumnDef::new(PlanShare::RecipientId).integer().null())
                .col(ColumnDef::new(PlanShare::Token).string_len(64).null().unique_key())
                .col(ColumnDef::new(PlanShare::Label).string_len(120).not_null().default(""))
                .col(ColumnDef::new(PlanShare::CreatedAt).date_time().not_null())
                .col(ColumnDef::new(PlanShare::ExpiresAt).date_time().null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_plan_share_owner")
                        .from(PlanShare::Table, PlanShare::OwnerId)
                        .to(AppUser::Table, AppUser::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_plan_share_recipient")
                        .from(PlanShare::Table, PlanShare::RecipientId)
                        .to(AppUser::Table, AppUser::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ix_plan_share_owner")
                .table(PlanShare::Table)
                .col(PlanShare::OwnerId)
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ix_plan_share_recipient")
                .table(PlanShare::Table)
                .col(PlanShare::RecipientId)
                .to_owned(),
        )
        .await?;

        m.create_table(
            Table::create()
                .table(PlanShareSession::Table)
                .if_not_exists()
                .col(ColumnDef::new(PlanShareSession::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(PlanShareSession::ShareId).integer().not_null())
                .col(ColumnDef::new(PlanShareSession::SessionId).integer().not_null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_plan_share_session_share")
                        .from(PlanShareSession::Table, PlanShareSession::ShareId)
                        .to(PlanShare::Table, PlanShare::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_plan_share_session_session")
                        .from(PlanShareSession::Table, PlanShareSession::SessionId)
                        .to(PlanSession::Table, PlanSession::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ux_plan_share_session")
                .table(PlanShareSession::Table)
                .col(PlanShareSession::ShareId)
                .col(PlanShareSession::SessionId)
                .unique()
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ix_plan_share_session_session")
                .table(PlanShareSession::Table)
                .col(PlanShareSession::SessionId)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.drop_table(Table::drop().table(PlanShareSession::Table).to_owned()).await?;
        m.drop_table(Table::drop().table(PlanShare::Table).to_owned()).await
    }
}
