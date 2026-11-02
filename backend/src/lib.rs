//! CaiRezekiYojana - a multi-currency investment tracker.
//!
//! Layering, outermost first:
//!
//! - `api`       HTTP handlers. Thin: parse, delegate, serialise.
//! - `services`  Orchestration that touches the database.
//! - `domain`    Pure financial logic. No IO, and where the tests live.
//! - `providers` Outbound feeds (quotes, NAV, FX), each behind a trait.
//! - `importer`  Spreadsheet ingestion.
//! - `entities`  SeaORM models.
//! - `migration` Schema, portable across SQLite and Postgres.

pub mod api;
pub mod auth;
pub mod config;
pub mod domain;
pub mod entities;
pub mod error;
pub mod importer;
pub mod migration;
pub mod providers;
pub mod services;
pub mod state;
pub mod types;
pub mod uploads;

use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;

use crate::config::Config;
use crate::error::AppResult;

/// Open the database, creating the SQLite file and its directory if needed.
pub async fn connect(config: &Config) -> AppResult<DatabaseConnection> {
    if config.is_sqlite() {
        ensure_sqlite_dir(&config.database_url)?;
        ensure_sqlite_tmpdir(&config.database_url)?;
    }

    let mut opts = ConnectOptions::new(config.database_url.clone());
    opts.max_connections(if config.is_sqlite() { 4 } else { 16 })
        .min_connections(1)
        .connect_timeout(std::time::Duration::from_secs(10))
        .sqlx_logging(false);

    Ok(Database::connect(opts).await?)
}

pub async fn migrate(db: &DatabaseConnection) -> AppResult<()> {
    migration::Migrator::up(db, None).await?;
    Ok(())
}

/// `sqlite://./data/cairezeki.db?mode=rwc` needs `./data` to exist first;
/// SQLite will create the file but not the directory.
fn ensure_sqlite_dir(url: &str) -> AppResult<()> {
    let Some(path) = config::sqlite_file_path(url) else {
        return Ok(());
    };
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

/// SQLite spills sorts and large transactions to temp files, looking in
/// `SQLITE_TMPDIR`, then `TMPDIR`, then `/var/tmp`, `/usr/tmp` and `/tmp`.
/// In a container with a read-only root filesystem none of those is
/// writable, and once the price table is big enough to need a spill every
/// query on it fails with "disk I/O error" (extended code 6410,
/// `SQLITE_IOERR_GETTEMPPATH`). Point SQLite at a `tmp/` folder beside the
/// database - the volume is the one place that is always writable - unless
/// the operator has set `SQLITE_TMPDIR` already.
fn ensure_sqlite_tmpdir(url: &str) -> AppResult<()> {
    if std::env::var_os("SQLITE_TMPDIR").is_some_and(|v| !v.is_empty()) {
        return Ok(());
    }
    let Some(dir) = config::sqlite_sibling_dir(url, "tmp") else {
        return Ok(());
    };
    std::fs::create_dir_all(&dir)?;
    std::env::set_var("SQLITE_TMPDIR", &dir);
    tracing::info!(dir = %dir, "SQLite temp files");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_directory_from_a_sqlite_url() {
        // Nothing to create for an in-memory or bare-filename database.
        assert!(ensure_sqlite_dir("sqlite::memory:").is_ok());
        assert!(ensure_sqlite_dir("sqlite://cairezeki.db").is_ok());
        assert!(ensure_sqlite_dir("postgres://localhost/x").is_ok());
    }
}
