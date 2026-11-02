//! Schema migrations.
//!
//! The same migration code targets SQLite and Postgres - SeaORM's schema
//! builder emits the right DDL per backend. Run automatically at boot
//! (see `CAIREZEKI_AUTO_MIGRATE`) or manually with `cairezeki migrate`.

pub use sea_orm_migration::prelude::*;

mod idens;
mod m20260101_000001_create_schema;
mod m20260101_000002_seed_reference;
mod m20260101_000003_seed_performance_settings;
mod m20260101_000004_history_window_and_year_repair;
mod m20260101_000005_fund_holdings;
mod m20260101_000006_fx_fees;
mod m20260101_000007_upload_audit;
mod m20260101_000008_bank_balances_off_deposit_accounts;
pub mod m20260101_000009_users_and_profiles;
mod m20260101_000010_api_keys;
mod m20260101_000011_deposit_tds;
mod m20260101_000012_signup_approval;
mod m20260101_000013_signup_off;
mod m20260101_000014_plan_sessions;
mod m20260101_000015_plan_session_uploads;
mod m20260101_000016_dhan_yatra_sessions;
mod m20260101_000017_plan_shares;
mod m20260101_000018_ai_mode;
mod m20260101_000019_ai_chat;
mod m20260101_000020_ai_proposals;
mod m20260101_000021_ai_page_actions;
mod m20260101_000022_row_level_security;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260101_000001_create_schema::Migration),
            Box::new(m20260101_000002_seed_reference::Migration),
            Box::new(m20260101_000003_seed_performance_settings::Migration),
            Box::new(m20260101_000004_history_window_and_year_repair::Migration),
            Box::new(m20260101_000005_fund_holdings::Migration),
            Box::new(m20260101_000006_fx_fees::Migration),
            Box::new(m20260101_000007_upload_audit::Migration),
            Box::new(m20260101_000008_bank_balances_off_deposit_accounts::Migration),
            Box::new(m20260101_000009_users_and_profiles::Migration),
            Box::new(m20260101_000010_api_keys::Migration),
            Box::new(m20260101_000011_deposit_tds::Migration),
            Box::new(m20260101_000012_signup_approval::Migration),
            Box::new(m20260101_000013_signup_off::Migration),
            Box::new(m20260101_000014_plan_sessions::Migration),
            Box::new(m20260101_000015_plan_session_uploads::Migration),
            Box::new(m20260101_000016_dhan_yatra_sessions::Migration),
            Box::new(m20260101_000017_plan_shares::Migration),
            Box::new(m20260101_000018_ai_mode::Migration),
            Box::new(m20260101_000019_ai_chat::Migration),
            Box::new(m20260101_000020_ai_proposals::Migration),
            Box::new(m20260101_000021_ai_page_actions::Migration),
            Box::new(m20260101_000022_row_level_security::Migration),
        ]
    }
}
