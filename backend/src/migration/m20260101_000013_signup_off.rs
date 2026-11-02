//! The sign-up page is off unless an administrator turns it on.
//!
//! Earlier installs were seeded with `signup_enabled = true`, so the page
//! was open on every one of them. Turn it off once; an administrator who
//! wants it turns it back on, confirming with ENABLE.

use sea_orm_migration::prelude::*;

use super::idens::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.exec_stmt(
            Query::update()
                .table(AppSetting::Table)
                .value(AppSetting::Value, "false")
                .value(AppSetting::UpdatedAt, chrono::Utc::now().naive_utc())
                .and_where(Expr::col(AppSetting::Key).eq("signup_enabled"))
                .to_owned(),
        )
        .await
    }

    async fn down(&self, _: &SchemaManager) -> Result<(), DbErr> {
        // Which installs had it on deliberately is not known; leave it off.
        Ok(())
    }
}
