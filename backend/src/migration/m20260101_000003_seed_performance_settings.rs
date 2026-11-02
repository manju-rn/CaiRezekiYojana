//! Defaults for historical-NAV backfill and period-return configuration.

use sea_orm_migration::prelude::*;

use super::idens::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const SETTINGS: &[(&str, &str)] = &[
    ("history_backfill_enabled", "true"),
    ("performance_periods", "1d,1w,1m,3m,6m,1y"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        let now = chrono::Utc::now().naive_utc();

        let mut st = Query::insert();
        st.into_table(AppSetting::Table).columns([
            AppSetting::Key,
            AppSetting::Value,
            AppSetting::UpdatedAt,
        ]);
        for (k, v) in SETTINGS {
            st.values_panic([(*k).into(), (*v).into(), now.into()]);
        }
        m.exec_stmt(st.to_owned()).await?;

        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.exec_stmt(
            Query::delete()
                .from_table(AppSetting::Table)
                .and_where(Expr::col(AppSetting::Key).is_in(["history_backfill_enabled", "performance_periods"]))
                .to_owned(),
        )
        .await?;
        Ok(())
    }
}
