//! Bank balances filed under the wrong account.
//!
//! Until this release the importer matched an account by name alone, so a
//! bank balance for "HDFC" landed on a deposit account of that name when the
//! deposits file had been imported first. Cash lists bank accounts
//! only, so those balances never showed. Each one moves here to a bank
//! account of the same name, created from the account it was filed under
//! where there is none yet.

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::ConnectionTrait;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// A non-bank account holding a balance, and a bank account of its name.
const MISFILED: &str = "account.kind <> 'BANK' \
    AND EXISTS (SELECT 1 FROM cash_balance c WHERE c.account_id = account.id)";

/// In order: create the missing bank accounts, drop a misfiled balance where
/// the bank account already has one for that day (unique per day; the bank
/// account's own is kept), then move the rest across.
const STATEMENTS: &[&str] = &[
    "INSERT INTO account \
        (name, kind, country, currency_code, institution, account_ref, opened_on, \
         is_active, notes, created_at, updated_at) \
     SELECT name, 'BANK', country, currency_code, institution, NULL, NULL, \
         is_active, 'Bank balances moved here from a deposit or investment account of the same name', \
         created_at, updated_at \
     FROM account \
     WHERE {MISFILED} \
       AND NOT EXISTS (SELECT 1 FROM account b \
                       WHERE b.kind = 'BANK' AND lower(b.name) = lower(account.name))",
    "DELETE FROM cash_balance \
     WHERE account_id IN (SELECT id FROM account WHERE kind <> 'BANK') \
       AND EXISTS (SELECT 1 FROM cash_balance q \
                   JOIN account b ON b.id = q.account_id \
                   JOIN account a ON a.id = cash_balance.account_id \
                   WHERE b.kind = 'BANK' AND lower(b.name) = lower(a.name) \
                     AND q.as_of = cash_balance.as_of)",
    "UPDATE cash_balance \
     SET account_id = (SELECT min(b.id) FROM account b \
                       JOIN account a ON lower(b.name) = lower(a.name) \
                       WHERE b.kind = 'BANK' AND a.id = cash_balance.account_id) \
     WHERE account_id IN (SELECT id FROM account WHERE kind <> 'BANK')",
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        let db = m.get_connection();
        for sql in STATEMENTS {
            db.execute_unprepared(&sql.replace("{MISFILED}", MISFILED)).await?;
        }
        Ok(())
    }

    async fn down(&self, _m: &SchemaManager) -> Result<(), DbErr> {
        // The balances stay where they belong.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sea_orm_migration::sea_orm::{ActiveModelTrait, Database, EntityTrait, Set};
    use sea_orm_migration::MigratorTrait;

    use crate::entities::{account, cash_balance};
    use crate::migration::Migrator;

    /// An account as the schema stood before this migration - written in SQL,
    /// since the entity has since grown columns the table did not have yet.
    async fn new_account(db: &sea_orm::DatabaseConnection, name: &str, kind: &str) -> i32 {
        use sea_orm_migration::sea_orm::ConnectionTrait;
        let now = chrono::Utc::now().naive_utc();
        db.execute_unprepared(&format!(
            "INSERT INTO account (name, kind, country, currency_code, institution, is_active, created_at, updated_at) \
             VALUES ('{name}', '{kind}', 'IN', 'INR', '{name} Bank', 1, '{now}', '{now}')"
        ))
        .await
        .unwrap()
        .last_insert_id() as i32
    }

    fn balance(account_id: i32, day: u32, amount: f64) -> cash_balance::ActiveModel {
        cash_balance::ActiveModel {
            account_id: Set(account_id),
            as_of: Set(chrono::NaiveDate::from_ymd_opt(2026, 9, day).unwrap()),
            balance: Set(amount),
            currency_code: Set("INR".into()),
            source: Set("import".into()),
            notes: Set(None),
            created_at: Set(chrono::Utc::now().naive_utc()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn balances_on_deposit_accounts_move_to_a_bank_account_of_that_name() {
        let path = std::env::temp_dir().join(format!("cairezeki-misfiled-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Database::connect(format!("sqlite://{}?mode=rwc", path.display()))
            .await
            .unwrap();
        Migrator::up(&db, Some(7)).await.unwrap();

        // HDFC: deposit account only. Kotak: a bank account exists too, with
        // its own balance on the 20th.
        let hdfc = new_account(&db, "HDFC", "DEPOSIT").await;
        let kotak_fd = new_account(&db, "KOTAK", "DEPOSIT").await;
        let kotak = new_account(&db, "Kotak", "BANK").await;
        balance(hdfc, 20, 879.0).insert(&db).await.unwrap();
        balance(kotak_fd, 20, 343.0).insert(&db).await.unwrap();
        balance(kotak_fd, 21, 344.0).insert(&db).await.unwrap();
        balance(kotak, 20, 500.0).insert(&db).await.unwrap();

        Migrator::up(&db, None).await.unwrap();

        let accounts = account::Entity::find().all(&db).await.unwrap();
        let banks: Vec<_> = accounts.iter().filter(|a| a.kind == "BANK").collect();
        assert_eq!(banks.len(), 2, "one new bank account, for HDFC");
        let hdfc_bank = banks.iter().find(|a| a.name == "HDFC").unwrap();
        assert_eq!(hdfc_bank.institution.as_deref(), Some("HDFC Bank"));

        let balances = cash_balance::Entity::find().all(&db).await.unwrap();
        assert!(balances.iter().all(|b| b.account_id == hdfc_bank.id || b.account_id == kotak));
        let kotak_rows: Vec<f64> = balances
            .iter()
            .filter(|b| b.account_id == kotak)
            .map(|b| b.balance)
            .collect();
        assert_eq!(kotak_rows.len(), 2);
        assert!(kotak_rows.contains(&500.0), "the bank account's own balance wins its day");
        assert!(kotak_rows.contains(&344.0));

        drop(db);
        let _ = std::fs::remove_file(&path);
    }
}
