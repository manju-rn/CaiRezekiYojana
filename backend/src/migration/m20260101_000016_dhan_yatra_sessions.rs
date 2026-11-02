//! CaiYatra keeps recorded sessions as the planner does, in the same
//! table: `kind` says whose a session is, `planner` or `dhan_yatra`. Every
//! session kept before is the planner's.

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
                .add_column(ColumnDef::new(PlanSession::Kind).string_len(20).not_null().default("planner"))
                .to_owned(),
        )
        .await
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.alter_table(Table::alter().table(PlanSession::Table).drop_column(PlanSession::Kind).to_owned()).await
    }
}
