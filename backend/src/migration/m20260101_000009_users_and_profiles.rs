//! Users, and every record belonging to one of them.
//!
//! Until this release there was one implicit user. Now there are three
//! kinds of account - `ADMIN`, who manages accounts and the app-wide
//! settings but has no portfolio; `USER`, who has one; and `EXPORTER`, who
//! can read every user's records but only to export them.
//!
//! 1. The account tables: `app_user`, the single-use tokens that activate an
//!    account or reset its password (`user_token`), OAuth logins
//!    (`user_identity`), browser sessions (`user_session`) and each user's
//!    own preferences (`user_setting`).
//!
//! 2. An owner on each table holding a user's own records: `account` (and
//!    through it transactions, deposits and balances), `import_batch`,
//!    `upload_audit` and `fx_setting`. Instruments, prices, rates, fees and
//!    fund portfolios are market data and stay shared.
//!
//! 3. A database already holding records gets an `Owner` profile to hold
//!    them, with the preferences that used to be app-wide. It has no
//!    password: an administrator issues it an activation token, and whoever
//!    redeems it sets one.

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, Statement};

use super::idens::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// Where the records of a pre-accounts database are put.
pub const OWNER_EMAIL: &str = "owner@cairezeki.local";

/// Preferences that were app-wide and are now each user's own.
const USER_KEYS: &[&str] = &[
    "base_currency",
    "cost_basis_method",
    "financial_year_start_month",
    "performance_periods",
    "visibility_india_equity",
    "visibility_india_mf",
    "visibility_global",
    "visibility_deposits",
    "visibility_cash",
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.create_table(
            Table::create()
                .table(AppUser::Table)
                .if_not_exists()
                .col(ColumnDef::new(AppUser::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(AppUser::Email).string_len(254).not_null().unique_key())
                .col(ColumnDef::new(AppUser::DisplayName).string_len(120).not_null())
                .col(ColumnDef::new(AppUser::Role).string_len(16).not_null())
                .col(ColumnDef::new(AppUser::PasswordHash).string_len(255).null())
                .col(ColumnDef::new(AppUser::EmailVerified).boolean().not_null().default(false))
                .col(ColumnDef::new(AppUser::IsEnabled).boolean().not_null().default(true))
                .col(ColumnDef::new(AppUser::LockedUntil).date_time().null())
                .col(ColumnDef::new(AppUser::LockReason).text().null())
                .col(ColumnDef::new(AppUser::MustSetPassword).boolean().not_null().default(false))
                .col(ColumnDef::new(AppUser::CreatedAt).date_time().not_null())
                .col(ColumnDef::new(AppUser::UpdatedAt).date_time().not_null())
                .col(ColumnDef::new(AppUser::LastLoginAt).date_time().null())
                .to_owned(),
        )
        .await?;

        m.create_table(
            Table::create()
                .table(UserToken::Table)
                .if_not_exists()
                .col(ColumnDef::new(UserToken::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(UserToken::UserId).integer().not_null())
                .col(ColumnDef::new(UserToken::Purpose).string_len(16).not_null())
                .col(ColumnDef::new(UserToken::TokenHash).string_len(64).not_null().unique_key())
                .col(ColumnDef::new(UserToken::ExpiresAt).date_time().not_null())
                .col(ColumnDef::new(UserToken::UsedAt).date_time().null())
                .col(ColumnDef::new(UserToken::CreatedAt).date_time().not_null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_user_token_user")
                        .from(UserToken::Table, UserToken::UserId)
                        .to(AppUser::Table, AppUser::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

        m.create_table(
            Table::create()
                .table(UserIdentity::Table)
                .if_not_exists()
                .col(ColumnDef::new(UserIdentity::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(UserIdentity::UserId).integer().not_null())
                .col(ColumnDef::new(UserIdentity::Provider).string_len(32).not_null())
                .col(ColumnDef::new(UserIdentity::Subject).string_len(255).not_null())
                .col(ColumnDef::new(UserIdentity::Email).string_len(254).null())
                .col(ColumnDef::new(UserIdentity::CreatedAt).date_time().not_null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_user_identity_user")
                        .from(UserIdentity::Table, UserIdentity::UserId)
                        .to(AppUser::Table, AppUser::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ux_user_identity_provider_subject")
                .table(UserIdentity::Table)
                .col(UserIdentity::Provider)
                .col(UserIdentity::Subject)
                .unique()
                .to_owned(),
        )
        .await?;

        m.create_table(
            Table::create()
                .table(UserSession::Table)
                .if_not_exists()
                .col(ColumnDef::new(UserSession::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(UserSession::UserId).integer().not_null())
                .col(ColumnDef::new(UserSession::TokenHash).string_len(64).not_null().unique_key())
                .col(ColumnDef::new(UserSession::CreatedAt).date_time().not_null())
                .col(ColumnDef::new(UserSession::ExpiresAt).date_time().not_null())
                .col(ColumnDef::new(UserSession::LastSeenAt).date_time().not_null())
                .col(ColumnDef::new(UserSession::UserAgent).string_len(255).null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_user_session_user")
                        .from(UserSession::Table, UserSession::UserId)
                        .to(AppUser::Table, AppUser::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

        m.create_table(
            Table::create()
                .table(UserSetting::Table)
                .if_not_exists()
                .col(ColumnDef::new(UserSetting::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(UserSetting::UserId).integer().not_null())
                .col(ColumnDef::new(UserSetting::Key).string_len(64).not_null())
                .col(ColumnDef::new(UserSetting::Value).text().not_null())
                .col(ColumnDef::new(UserSetting::UpdatedAt).date_time().not_null())
                .foreign_key(
                    ForeignKey::create()
                        .name("fk_user_setting_user")
                        .from(UserSetting::Table, UserSetting::UserId)
                        .to(AppUser::Table, AppUser::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        m.create_index(
            Index::create()
                .name("ux_user_setting_user_key")
                .table(UserSetting::Table)
                .col(UserSetting::UserId)
                .col(UserSetting::Key)
                .unique()
                .to_owned(),
        )
        .await?;

        // SQLite cannot add a foreign key to an existing table, so the owner
        // columns are plain integers; the app deletes a user's records itself.
        for table in [
            Account::Table.into_iden(),
            ImportBatch::Table.into_iden(),
            UploadAudit::Table.into_iden(),
            FxSetting::Table.into_iden(),
        ] {
            m.alter_table(
                Table::alter()
                    .table(table)
                    .add_column(ColumnDef::new(Owner::UserId).integer().null())
                    .to_owned(),
            )
            .await?;
        }

        let db = m.get_connection();
        let backend = db.get_database_backend();
        let count = |sql: &str| {
            let sql = sql.to_string();
            async move {
                let row = db.query_one(Statement::from_string(backend, sql)).await?;
                Ok::<i64, DbErr>(row.map(|r| r.try_get_by_index::<i64>(0).unwrap_or(0)).unwrap_or(0))
            }
        };
        let has_records = count("SELECT COUNT(*) FROM account").await?
            + count("SELECT COUNT(*) FROM import_batch").await?
            + count("SELECT COUNT(*) FROM upload_audit").await?
            > 0;

        if has_records {
            let now = chrono::Utc::now().naive_utc();
            m.exec_stmt(
                Query::insert()
                    .into_table(AppUser::Table)
                    .columns([
                        AppUser::Email,
                        AppUser::DisplayName,
                        AppUser::Role,
                        AppUser::EmailVerified,
                        AppUser::IsEnabled,
                        AppUser::MustSetPassword,
                        AppUser::CreatedAt,
                        AppUser::UpdatedAt,
                    ])
                    .values_panic([
                        OWNER_EMAIL.into(),
                        "Owner".into(),
                        "USER".into(),
                        true.into(),
                        true.into(),
                        true.into(),
                        now.into(),
                        now.into(),
                    ])
                    .to_owned(),
            )
            .await?;
            let owner = count(&format!("SELECT id FROM app_user WHERE email = '{OWNER_EMAIL}'")).await?;

            for table in ["account", "import_batch", "upload_audit", "fx_setting"] {
                db.execute_unprepared(&format!("UPDATE {table} SET user_id = {owner} WHERE user_id IS NULL"))
                    .await?;
            }
            let keys = USER_KEYS.iter().map(|k| format!("'{k}'")).collect::<Vec<_>>().join(", ");
            db.execute_unprepared(&format!(
                "INSERT INTO user_setting (user_id, key, value, updated_at) \
                 SELECT {owner}, key, value, updated_at FROM app_setting WHERE key IN ({keys})"
            ))
            .await?;
        } else {
            // Only the seeded pairs: each new user gets their own as needed.
            db.execute_unprepared("DELETE FROM fx_setting").await?;
        }

        // One policy per pair, now per user.
        m.drop_index(Index::drop().name("ux_fx_setting_pair").table(FxSetting::Table).to_owned()).await?;
        m.create_index(
            Index::create()
                .name("ux_fx_setting_user_pair")
                .table(FxSetting::Table)
                .col(Owner::UserId)
                .col(FxSetting::BaseCode)
                .col(FxSetting::QuoteCode)
                .unique()
                .to_owned(),
        )
        .await?;
        for (name, table) in [
            ("ix_account_user", Account::Table.into_iden()),
            ("ix_import_batch_user", ImportBatch::Table.into_iden()),
            ("ix_upload_audit_user", UploadAudit::Table.into_iden()),
        ] {
            m.create_index(Index::create().name(name).table(table).col(Owner::UserId).to_owned()).await?;
        }

        // App-wide defaults for the new account settings.
        let now = chrono::Utc::now().naive_utc();
        let mut insert = Query::insert();
        insert.into_table(AppSetting::Table).columns([AppSetting::Key, AppSetting::Value, AppSetting::UpdatedAt]);
        for (k, v) in [
            ("signup_enabled", "false"),
            ("email_verification_required", "false"),
            ("oauth_auto_register", "true"),
            ("default_base_currency", "SGD"),
            ("session_days", "30"),
        ] {
            insert.values_panic([k.into(), v.into(), now.into()]);
        }
        m.exec_stmt(insert.to_owned()).await?;

        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        for t in [
            UserSetting::Table.into_iden(),
            UserSession::Table.into_iden(),
            UserIdentity::Table.into_iden(),
            UserToken::Table.into_iden(),
            AppUser::Table.into_iden(),
        ] {
            m.drop_table(Table::drop().table(t).if_exists().to_owned()).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm_migration::sea_orm::{ActiveModelTrait, ColumnTrait, Database, EntityTrait, QueryFilter, Set};
    use sea_orm_migration::MigratorTrait;

    use crate::entities::{account, app_user, fx_setting, user_setting};
    use crate::migration::Migrator;

    async fn fresh_db(tag: &str) -> (sea_orm::DatabaseConnection, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("cairezeki-users-{tag}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Database::connect(format!("sqlite://{}?mode=rwc", path.display())).await.unwrap();
        (db, path)
    }

    #[tokio::test]
    async fn existing_records_go_to_an_owner_profile() {
        let (db, path) = fresh_db("owner").await;
        Migrator::up(&db, Some(8)).await.unwrap();

        let now = chrono::Utc::now().naive_utc();
        let before = sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            format!(
                "INSERT INTO account (name, kind, country, currency_code, is_active, created_at, updated_at) \
                 VALUES ('Zerodha', 'DEMAT', 'IN', 'INR', 1, '{now}', '{now}')"
            ),
        );
        sea_orm::ConnectionTrait::execute(&db, before).await.unwrap();
        sea_orm::ConnectionTrait::execute_unprepared(
            &db,
            "UPDATE app_setting SET value = 'SGD' WHERE key = 'base_currency'",
        )
        .await
        .unwrap();

        Migrator::up(&db, None).await.unwrap();

        let owner = app_user::Entity::find()
            .filter(app_user::Column::Email.eq(super::OWNER_EMAIL))
            .one(&db)
            .await
            .unwrap()
            .expect("an owner profile");
        assert_eq!(owner.role, "USER");
        assert!(owner.password_hash.is_none(), "activated by token, never by a set password");
        let accounts = account::Entity::find().all(&db).await.unwrap();
        assert!(accounts.iter().all(|a| a.user_id == owner.id));
        let base = user_setting::Entity::find()
            .filter(user_setting::Column::UserId.eq(owner.id))
            .filter(user_setting::Column::Key.eq("base_currency"))
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(base.value, "SGD");
        let pairs = fx_setting::Entity::find().all(&db).await.unwrap();
        assert!(!pairs.is_empty() && pairs.iter().all(|p| p.user_id == owner.id), "seeded pairs kept, owned");

        drop(db);
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn an_empty_database_gets_no_owner() {
        let (db, path) = fresh_db("empty").await;
        Migrator::up(&db, None).await.unwrap();
        assert!(app_user::Entity::find().all(&db).await.unwrap().is_empty());
        assert!(fx_setting::Entity::find().all(&db).await.unwrap().is_empty());

        // A second user may hold the same pair as another.
        let now = chrono::Utc::now().naive_utc();
        for email in ["a@x.io", "b@x.io"] {
            let u = app_user::ActiveModel {
                email: Set(email.into()),
                display_name: Set(email.into()),
                role: Set("USER".into()),
                password_hash: Set(None),
                email_verified: Set(true),
                is_enabled: Set(true),
                locked_until: Set(None),
                lock_reason: Set(None),
                must_set_password: Set(false),
                created_at: Set(now),
                updated_at: Set(now),
                last_login_at: Set(None),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
            fx_setting::ActiveModel {
                user_id: Set(u.id),
                base_code: Set("USD".into()),
                quote_code: Set("INR".into()),
                provider: Set("default".into()),
                margin_mode: Set("NONE".into()),
                margin_value: Set(0.0),
                manual_rate: Set(None),
                use_manual: Set(false),
                note: Set(None),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(&db)
            .await
            .unwrap();
        }

        drop(db);
        let _ = std::fs::remove_file(&path);
    }
}
