//! Two changes that arrived together with the ten-year NAV chart.
//!
//! 1. `instrument.history_from`: the earliest date a history feed has been
//!    asked to fill `price_point` back to for this instrument. The backfill
//!    skips an instrument whose marker already reaches what it needs. The
//!    stored rows cannot make that call on their own - a fund launched six
//!    years ago has no ten-year history to reach, and would be fetched
//!    again every night.
//!
//! 2. Repair of two-digit years. Until this release the importer read
//!    `29-May-17` as the year 17, so every date on a Value Research
//!    statement landed in the first century. A date before the year 100
//!    cannot be anything else, so it is widened here with the pivot the
//!    parser now applies: 00-69 -> 2000s, 70-99 -> 1900s.

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, DbBackend};

use super::idens::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// Every `(table, column)` the importer writes a parsed date into.
const DATE_COLUMNS: &[(&str, &str)] = &[
    ("txn", "trade_date"),
    ("price_point", "as_of"),
    ("cash_balance", "as_of"),
    ("deposit", "start_date"),
    ("deposit", "maturity_date"),
    ("account", "opened_on"),
];

/// Tables with a unique `(owner, as_of)` index, where a widened row could
/// land on a day that has since been written correctly.
const UNIQUE_BY_DAY: &[(&str, &str)] = &[
    ("price_point", "instrument_id"),
    ("cash_balance", "account_id"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        m.alter_table(
            Table::alter()
                .table(Instrument::Table)
                .add_column(ColumnDef::new(Instrument::HistoryFrom).date().null())
                .to_owned(),
        )
        .await?;

        let db = m.get_connection();
        for sql in repair_statements(m.get_database_backend()) {
            db.execute_unprepared(&sql).await?;
        }
        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        // The year repair stays: the dates it replaced were never right.
        m.alter_table(
            Table::alter()
                .table(Instrument::Table)
                .drop_column(Instrument::HistoryFrom)
                .to_owned(),
        )
        .await
    }
}

/// The statements, in order: first drop the broken rows that would collide
/// with a correct one once widened - the correct row (a feed NAV, a later
/// balance) is the better of the two - then widen everything else.
fn repair_statements(backend: DbBackend) -> Vec<String> {
    let mut out = Vec::new();
    for (table, owner) in UNIQUE_BY_DAY {
        let col = format!("{table}.as_of");
        out.push(format!(
            "DELETE FROM {table} WHERE {broken} AND EXISTS (\
                SELECT 1 FROM {table} q WHERE q.{owner} = {table}.{owner} AND q.as_of = {widened})",
            broken = broken(backend, &col),
            widened = widened(backend, &col),
        ));
    }
    for (table, col) in DATE_COLUMNS {
        out.push(format!(
            "UPDATE {table} SET {col} = {widened} WHERE {broken}",
            widened = widened(backend, col),
            broken = broken(backend, col),
        ));
    }
    out
}

/// Test for a date read with a two-digit year, i.e. one before the year 100.
/// SQLite keeps a DATE as `YYYY-MM-DD` text, so it is a prefix test there.
fn broken(backend: DbBackend, col: &str) -> String {
    match backend {
        DbBackend::Sqlite => format!("{col} LIKE '00__-%'"),
        _ => format!("{col} < DATE '0100-01-01'"),
    }
}

/// `col` with its century restored: text surgery on SQLite, interval
/// arithmetic elsewhere. Years 70-99 become the 1900s and 00-69 the 2000s,
/// which is chrono's own `%y` pivot.
fn widened(backend: DbBackend, col: &str) -> String {
    match backend {
        DbBackend::Sqlite => format!(
            "(CASE WHEN substr({col}, 3, 2) >= '70' THEN '19' ELSE '20' END) || substr({col}, 3)"
        ),
        _ => format!(
            "({col} + (CASE WHEN {col} >= DATE '0070-01-01' \
                THEN INTERVAL '1900 years' ELSE INTERVAL '2000 years' END))::date"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_repair_is_text_surgery_on_the_stored_iso_date() {
        let sql = repair_statements(DbBackend::Sqlite);
        assert_eq!(sql.len(), UNIQUE_BY_DAY.len() + DATE_COLUMNS.len());
        assert!(sql[0].starts_with("DELETE FROM price_point WHERE price_point.as_of LIKE '00__-%'"));
        assert_eq!(
            sql[UNIQUE_BY_DAY.len()],
            "UPDATE txn SET trade_date = (CASE WHEN substr(trade_date, 3, 2) >= '70' THEN '19' ELSE '20' END) \
             || substr(trade_date, 3) WHERE trade_date LIKE '00__-%'"
        );
    }

    /// The repair run for real against SQLite: the schema as it stood before
    /// this migration, rows dated the way the old parser wrote them, then
    /// the migration.
    #[tokio::test]
    async fn sqlite_repair_widens_stored_dates_and_yields_to_correct_rows() {
        use sea_orm_migration::sea_orm::{Database, EntityTrait, QueryOrder};
        use sea_orm_migration::MigratorTrait;

        use crate::entities::{instrument, price_point, txn};
        use crate::migration::Migrator;

        let path = std::env::temp_dir().join(format!("cairezeki-repair-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Database::connect(format!("sqlite://{}?mode=rwc", path.display()))
            .await
            .unwrap();

        // Everything up to, but not including, this migration.
        Migrator::up(&db, Some(3)).await.unwrap();
        for sql in [
            "INSERT INTO account (id, name, kind, country, currency_code, created_at, updated_at) \
             VALUES (1, 'Folio', 'MF_FOLIO', 'IN', 'INR', '2026-01-01 00:00:00', '2026-01-01 00:00:00')",
            "INSERT INTO instrument (id, kind, symbol, name, currency_code, created_at, updated_at) \
             VALUES (1, 'MUTUAL_FUND', 'INF000000001', 'A Fund', 'INR', '2026-01-01 00:00:00', \
             '2026-01-01 00:00:00')",
            // 29-May-17 and 03-Sep-26 as the old parser stored them, and one
            // date that was always right.
            "INSERT INTO txn (account_id, instrument_id, trade_date, kind, currency_code, created_at) \
             VALUES (1, 1, '0017-05-29', 'BUY', 'INR', '2026-01-01 00:00:00'), \
                    (1, 1, '0026-09-03', 'BUY', 'INR', '2026-01-01 00:00:00'), \
                    (1, 1, '2026-09-10', 'BUY', 'INR', '2026-01-01 00:00:00')",
            // The statement NAV for 3 Sep 2026 collides with the feed NAV the
            // backfill has since stored for that day; 31-12-99 is the 1900s.
            "INSERT INTO price_point (instrument_id, as_of, close, currency_code, source, created_at) \
             VALUES (1, '0026-09-03', 100.0, 'INR', 'import', '2026-01-01 00:00:00'), \
                    (1, '2026-09-03', 101.0, 'INR', 'mfapi_history', '2026-01-01 00:00:00'), \
                    (1, '0017-05-29', 50.0, 'INR', 'import', '2026-01-01 00:00:00'), \
                    (1, '0099-12-31', 10.0, 'INR', 'import', '2026-01-01 00:00:00')",
        ] {
            db.execute_unprepared(sql).await.unwrap();
        }

        Migrator::up(&db, None).await.unwrap();

        let trades: Vec<String> = txn::Entity::find()
            .order_by_asc(txn::Column::TradeDate)
            .all(&db)
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.trade_date.to_string())
            .collect();
        assert_eq!(trades, ["2017-05-29", "2026-09-03", "2026-09-10"]);

        let prices: Vec<(String, String, f64)> = price_point::Entity::find()
            .order_by_asc(price_point::Column::AsOf)
            .all(&db)
            .await
            .unwrap()
            .into_iter()
            .map(|p| (p.as_of.to_string(), p.source, p.close))
            .collect();
        assert_eq!(
            prices,
            [
                ("1999-12-31".to_string(), "import".to_string(), 10.0),
                ("2017-05-29".to_string(), "import".to_string(), 50.0),
                ("2026-09-03".to_string(), "mfapi_history".to_string(), 101.0),
            ]
        );

        // The new column is there, and empty until a backfill fills it.
        let fund = instrument::Entity::find_by_id(1)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fund.history_from, None);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn postgres_repair_adds_the_missing_centuries() {
        let sql = repair_statements(DbBackend::Postgres);
        let update = &sql[UNIQUE_BY_DAY.len() + 1];
        assert!(update.starts_with(
            "UPDATE price_point SET as_of = (as_of + (CASE WHEN as_of >= DATE '0070-01-01'"
        ));
        assert!(update.ends_with("WHERE as_of < DATE '0100-01-01'"));
    }
}
