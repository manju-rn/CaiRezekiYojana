//! Deleting a user's records in bulk: one page's worth, one institution's,
//! both, or everything.
//!
//! Nothing deleted here can be brought back - there is no recycle bin and
//! no soft delete - so every entry point asks for [`CONFIRM_PHRASE`] typed
//! out, and the UI says so before it asks. Kept upload files stay on disk,
//! as nothing uploaded is ever deleted; instruments are shared market data
//! and stay too.

use std::collections::{BTreeMap, HashMap, HashSet};

use sea_orm::{
    ColumnTrait, DatabaseConnection, DatabaseTransaction, EntityTrait, PaginatorTrait, QueryFilter, TransactionTrait,
};
use serde::Serialize;

use crate::entities::{account, cash_balance, deposit, import_batch, instrument, txn};
use crate::error::{AppError, AppResult};

use super::cash::NO_INSTITUTION;
use super::portfolio::Scope;
use super::users;

/// What has to be typed, exactly, before anything is deleted.
pub const CONFIRM_PHRASE: &str = "DELETE";

/// The pages records can be deleted by, in menu order: `(key, label)`.
pub const CATEGORIES: &[(&str, &str)] = &[
    ("india_equity", "Indian stocks"),
    ("india_mf", "Indian mutual funds"),
    ("global", "Global stocks & ETFs"),
    ("deposits", "Deposits"),
    ("cash", "Cash"),
];

/// SQLite caps the parameters in one statement; stay well under it.
const CHUNK: usize = 500;

/// One page's records, and the institutions they are held at.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Category {
    /// `india_equity`, `india_mf`, `global`, `deposits` or `cash`.
    pub key: String,
    pub label: String,
    /// What `count` counts: `transactions`, `deposits` or `balances`.
    pub unit: String,
    pub count: usize,
    /// By name; empty when the page has nothing.
    pub institutions: Vec<InstitutionCount>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct InstitutionCount {
    /// The bank, broker or platform - or the account's name where no
    /// institution was recorded.
    pub name: String,
    pub count: usize,
}

/// What a delete removed.
#[derive(Debug, Default, Serialize, utoipa::ToSchema)]
pub struct Purged {
    pub transactions: u64,
    pub deposits: u64,
    pub cash_balances: u64,
    /// Accounts left with nothing in them, removed along with their records.
    pub accounts: u64,
    pub import_batches: u64,
}

/// Fails unless `typed` is exactly [`CONFIRM_PHRASE`].
pub fn check_confirmation(typed: &str) -> AppResult<()> {
    if typed.trim() != CONFIRM_PHRASE {
        return Err(AppError::bad(format!("type {CONFIRM_PHRASE} to confirm - deleted records cannot be restored")));
    }
    Ok(())
}

/// Whether `key` is one of [`CATEGORIES`].
pub fn check_category(key: &str) -> AppResult<()> {
    if CATEGORIES.iter().any(|(k, _)| *k == key) {
        Ok(())
    } else {
        let keys: Vec<&str> = CATEGORIES.iter().map(|(k, _)| *k).collect();
        Err(AppError::bad(format!("category must be one of: {}", keys.join(", "))))
    }
}

#[derive(Clone, Copy)]
enum Item {
    Txn(i32),
    Deposit(i32),
    Balance(i32),
}

/// One deletable record, with where it shows up.
struct Record {
    /// `None` for an instrument no page lists - an Indian bond, say. Those
    /// go only when every category is chosen.
    category: Option<&'static str>,
    institution: String,
    account_id: i32,
    item: Item,
}

/// The name an account's records are grouped under.
fn institution_of(a: &account::Model) -> String {
    a.institution
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| Some(a.name.trim()).filter(|s| !s.is_empty()))
        .unwrap_or(NO_INSTITUTION)
        .to_string()
}

/// The holdings page an instrument is listed on.
fn page_of(inst: &instrument::Model) -> Option<&'static str> {
    [(Scope::IndiaEquity, "india_equity"), (Scope::IndiaMf, "india_mf"), (Scope::Global, "global")]
        .into_iter()
        .find(|(scope, _)| scope.matches(inst))
        .map(|(_, key)| key)
}

/// Every record `owner` has, tagged with its page and institution.
async fn records(db: &DatabaseConnection, owner: i32) -> AppResult<Vec<Record>> {
    let accounts: HashMap<i32, account::Model> = account::Entity::find()
        .filter(account::Column::UserId.eq(owner))
        .all(db)
        .await?
        .into_iter()
        .map(|a| (a.id, a))
        .collect();
    let ids: Vec<i32> = accounts.keys().copied().collect();
    let mut out = Vec::new();

    let txns = txn::Entity::find().filter(txn::Column::AccountId.is_in(ids.clone())).all(db).await?;
    let instrument_ids: HashSet<i32> = txns.iter().map(|t| t.instrument_id).collect();
    let instruments: HashMap<i32, instrument::Model> = instrument::Entity::find()
        .filter(instrument::Column::Id.is_in(instrument_ids))
        .all(db)
        .await?
        .into_iter()
        .map(|i| (i.id, i))
        .collect();
    for t in txns {
        out.push(Record {
            category: instruments.get(&t.instrument_id).and_then(page_of),
            institution: institution_of(&accounts[&t.account_id]),
            account_id: t.account_id,
            item: Item::Txn(t.id),
        });
    }

    // A deposit carries its own institution, as the deposits page groups it.
    for d in deposit::Entity::find().filter(deposit::Column::AccountId.is_in(ids.clone())).all(db).await? {
        let name = d.institution.trim();
        out.push(Record {
            category: Some("deposits"),
            institution: if name.is_empty() { institution_of(&accounts[&d.account_id]) } else { name.to_string() },
            account_id: d.account_id,
            item: Item::Deposit(d.id),
        });
    }

    for b in cash_balance::Entity::find().filter(cash_balance::Column::AccountId.is_in(ids)).all(db).await? {
        out.push(Record {
            category: Some("cash"),
            institution: institution_of(&accounts[&b.account_id]),
            account_id: b.account_id,
            item: Item::Balance(b.id),
        });
    }
    Ok(out)
}

/// What `owner` could delete, page by page.
pub async fn inventory(db: &DatabaseConnection, owner: i32) -> AppResult<Vec<Category>> {
    let all = records(db, owner).await?;
    Ok(CATEGORIES
        .iter()
        .map(|(key, label)| {
            let mut by_bank: BTreeMap<&str, usize> = BTreeMap::new();
            for r in all.iter().filter(|r| r.category == Some(*key)) {
                *by_bank.entry(r.institution.as_str()).or_default() += 1;
            }
            Category {
                key: key.to_string(),
                label: label.to_string(),
                unit: match *key {
                    "deposits" => "deposits",
                    "cash" => "balances",
                    _ => "transactions",
                }
                .to_string(),
                count: by_bank.values().sum(),
                institutions: by_bank
                    .into_iter()
                    .map(|(name, count)| InstitutionCount { name: name.to_string(), count })
                    .collect(),
            }
        })
        .collect())
}

/// Delete `owner`'s records on one page (`None`: every page) at one
/// institution (`None`: every institution). Accounts this leaves empty go
/// too. Choosing neither is [`delete_everything`].
pub async fn delete(
    db: &DatabaseConnection,
    owner: i32,
    category: Option<&str>,
    institution: Option<&str>,
) -> AppResult<Purged> {
    if let Some(c) = category {
        check_category(c)?;
    }
    if category.is_none() && institution.is_none() {
        return delete_everything(db, owner).await;
    }

    let chosen: Vec<Record> = records(db, owner)
        .await?
        .into_iter()
        .filter(|r| category.map_or(true, |c| r.category == Some(c)))
        .filter(|r| institution.map_or(true, |i| r.institution.eq_ignore_ascii_case(i.trim())))
        .collect();

    let (mut txns, mut deposits, mut balances) = (Vec::new(), Vec::new(), Vec::new());
    for r in &chosen {
        match r.item {
            Item::Txn(id) => txns.push(id),
            Item::Deposit(id) => deposits.push(id),
            Item::Balance(id) => balances.push(id),
        }
    }
    let touched: HashSet<i32> = chosen.iter().map(|r| r.account_id).collect();

    let tx = db.begin().await?;
    let mut purged = Purged::default();
    for ids in txns.chunks(CHUNK) {
        purged.transactions += txn::Entity::delete_many().filter(txn::Column::Id.is_in(ids.to_vec())).exec(&tx).await?.rows_affected;
    }
    for ids in deposits.chunks(CHUNK) {
        purged.deposits +=
            deposit::Entity::delete_many().filter(deposit::Column::Id.is_in(ids.to_vec())).exec(&tx).await?.rows_affected;
    }
    for ids in balances.chunks(CHUNK) {
        purged.cash_balances += cash_balance::Entity::delete_many()
            .filter(cash_balance::Column::Id.is_in(ids.to_vec()))
            .exec(&tx)
            .await?
            .rows_affected;
    }
    for id in touched {
        if is_empty(&tx, id).await? {
            purged.accounts += account::Entity::delete_by_id(id).exec(&tx).await?.rows_affected;
        }
    }
    tx.commit().await?;
    tracing::info!(user = owner, ?category, ?institution, ?purged, "records deleted");
    Ok(purged)
}

/// Every record `owner` has: accounts with their transactions, deposits and
/// balances, and the import history. The account itself, its settings and
/// the upload audit stay.
pub async fn delete_everything(db: &DatabaseConnection, owner: i32) -> AppResult<Purged> {
    let ids = users::account_ids(db, owner).await?;
    let tx = db.begin().await?;
    // Counted first: the accounts take the rest with them by foreign key.
    let mut purged = Purged {
        transactions: txn::Entity::find().filter(txn::Column::AccountId.is_in(ids.clone())).count(&tx).await?,
        deposits: deposit::Entity::find().filter(deposit::Column::AccountId.is_in(ids.clone())).count(&tx).await?,
        cash_balances: cash_balance::Entity::find()
            .filter(cash_balance::Column::AccountId.is_in(ids))
            .count(&tx)
            .await?,
        ..Default::default()
    };
    purged.accounts = account::Entity::delete_many().filter(account::Column::UserId.eq(owner)).exec(&tx).await?.rows_affected;
    purged.import_batches =
        import_batch::Entity::delete_many().filter(import_batch::Column::UserId.eq(owner)).exec(&tx).await?.rows_affected;
    tx.commit().await?;
    tracing::info!(user = owner, ?purged, "all records deleted");
    Ok(purged)
}

/// Nothing left in the account.
async fn is_empty(db: &DatabaseTransaction, account_id: i32) -> AppResult<bool> {
    Ok(txn::Entity::find().filter(txn::Column::AccountId.eq(account_id)).count(db).await? == 0
        && deposit::Entity::find().filter(deposit::Column::AccountId.eq(account_id)).count(db).await? == 0
        && cash_balance::Entity::find().filter(cash_balance::Column::AccountId.eq(account_id)).count(db).await? == 0)
}
