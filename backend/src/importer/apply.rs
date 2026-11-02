//! Turning validated rows into database records.
//!
//! Imports are deliberately *partial*: good rows land, bad rows are reported
//! with their spreadsheet row number, and the batch records both. A single
//! mistyped date in row 400 should not cost you the other 399.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;

use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
};
use serde::Serialize;

use crate::entities::{account, cash_balance, deposit, exchange, import_batch, instrument, txn};
use crate::error::{AppError, AppResult};
use crate::services::refresh;
use crate::types::{
    AccountKind, AssetKind, Compounding, DepositKind, ImportKind, ImportStatus, PayoutFrequency,
    TxnKind,
};

use super::reader::{self, Row, Sheet};
use super::specs::{spec_for, ImportSpec};

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct RowError {
    /// Row number as it appears in the spreadsheet.
    pub row: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ImportOutcome {
    pub batch_id: Option<i32>,
    pub kind: String,
    pub filename: String,
    pub status: String,
    pub row_count: usize,
    pub ok_count: usize,
    /// Rows recognised as already imported.
    pub skipped_count: usize,
    pub error_count: usize,
    pub errors: Vec<RowError>,
    pub created_accounts: Vec<String>,
    pub created_instruments: Vec<String>,
    /// Headings in the file that no column spec claims. Harmless, but worth
    /// surfacing - a typo'd heading is the usual reason a column "did nothing".
    pub unrecognised_columns: Vec<String>,
    pub dry_run: bool,
    /// The name the file was kept under in the uploads folder. Both
    /// imports and validations are kept, when an uploads folder is configured.
    pub stored_as: Option<String>,
    /// This upload's number in the upload audit.
    pub audit_seq: Option<i32>,
}

/// Validate and (unless `dry_run`) apply an uploaded file to `owner`'s
/// records. Accounts are found or created among theirs alone.
pub async fn import(
    db: &DatabaseConnection,
    owner: i32,
    kind: ImportKind,
    filename: &str,
    bytes: &[u8],
    dry_run: bool,
) -> AppResult<ImportOutcome> {
    let spec = spec_for(kind);
    let sheet = reader::read(filename, bytes)?;
    check_required_columns(spec, &sheet)?;

    let unrecognised: Vec<String> = sheet
        .headers
        .iter()
        .filter(|h| !h.is_empty() && spec.canonical(h).is_none())
        .cloned()
        .collect();

    // Re-key every row onto canonical names so downstream code only ever
    // looks up one spelling.
    let rows: Vec<Row> = sheet
        .rows
        .iter()
        .map(|r| Row {
            number: r.number,
            cells: r
                .cells
                .iter()
                .filter_map(|(k, v)| spec.canonical(k).map(|c| (c.to_string(), v.clone())))
                .collect(),
        })
        // Statements end with a totals line ("Mutual Funds / SIFs Total"):
        // not a transaction, and not worth reporting as a failed row.
        .filter(|r| !is_summary_row(r))
        .collect();

    let mut resolver = Resolver::load(db, owner).await?;
    let mut errors = Vec::new();
    let mut ok = 0usize;
    let mut skipped = 0usize;

    // The batch row exists before the data rows so imported records can point
    // at it, which is what makes an undo possible.
    let batch_id = if dry_run {
        None
    } else {
        Some(create_batch(db, owner, kind, filename, rows.len()).await?)
    };

    for row in &rows {
        let result = match kind {
            ImportKind::IndiaEquity => {
                apply_security_txn(db, &mut resolver, row, batch_id, Market::India, dry_run).await
            }
            ImportKind::Global => {
                apply_security_txn(db, &mut resolver, row, batch_id, Market::Global, dry_run).await
            }
            ImportKind::IndiaMf => apply_mf_txn(db, &mut resolver, row, batch_id, dry_run).await,
            ImportKind::Deposits => apply_deposit(db, &mut resolver, row, dry_run).await,
            ImportKind::CashBalances => apply_cash(db, &mut resolver, row, dry_run).await,
            ImportKind::Instruments => apply_instrument(db, &mut resolver, row, dry_run).await,
        };

        match result {
            Ok(RowResult::Applied) => ok += 1,
            Ok(RowResult::Skipped) => skipped += 1,
            Err(e) => errors.push(RowError { row: row.number, message: e.to_string() }),
        }
    }

    let status = if !errors.is_empty() && ok == 0 {
        ImportStatus::Failed
    } else if !errors.is_empty() {
        ImportStatus::Partial
    } else {
        ImportStatus::Completed
    };

    if let Some(id) = batch_id {
        finalise_batch(db, id, status, ok, skipped, &errors).await?;
    }

    Ok(ImportOutcome {
        batch_id,
        kind: kind.to_string(),
        filename: filename.to_string(),
        status: status.to_string(),
        row_count: rows.len(),
        ok_count: ok,
        skipped_count: skipped,
        error_count: errors.len(),
        errors,
        created_accounts: resolver.created_accounts,
        created_instruments: resolver.created_instruments,
        unrecognised_columns: unrecognised,
        dry_run,
        stored_as: None,
        audit_seq: None,
    })
}

enum RowResult {
    Applied,
    Skipped,
}


/// What a transaction is, for recognising a row imported before when the
/// file gives no reference: the same account, instrument, day, type, units
/// and price. Figures are compared to a millionth, past float noise.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TxnPrint {
    account_id: i32,
    instrument_id: i32,
    date: chrono::NaiveDate,
    kind: String,
    quantity: i64,
    price: i64,
}

impl TxnPrint {
    fn new(account_id: i32, instrument_id: i32, date: chrono::NaiveDate, kind: &str, quantity: f64, price: f64) -> Self {
        Self {
            account_id,
            instrument_id,
            date,
            kind: kind.to_uppercase(),
            // Stored as a magnitude; the type carries the direction.
            quantity: millionths(quantity.abs()),
            price: millionths(price),
        }
    }
}

/// The same for a deposit: account, kind, principal, rate and term.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct DepositPrint {
    account_id: i32,
    kind: String,
    principal: i64,
    rate: i64,
    start: chrono::NaiveDate,
    maturity: chrono::NaiveDate,
}

impl DepositPrint {
    fn new(
        account_id: i32,
        kind: &str,
        principal: f64,
        rate: f64,
        start: chrono::NaiveDate,
        maturity: chrono::NaiveDate,
    ) -> Self {
        Self { account_id, kind: kind.to_uppercase(), principal: millionths(principal), rate: millionths(rate), start, maturity }
    }
}

/// Whether a row was imported before: by its reference when that is on
/// file, otherwise by its contents. Either way one copy on file is counted
/// matched, so identical trades told apart only by their references stay
/// separate.
fn seen_before<K: std::hash::Hash + Eq>(prints: &mut HashMap<K, usize>, print: K, known_ref: bool) -> bool {
    let matched = already_on_file(prints, &print);
    known_ref || matched
}

/// A totals line: its date cell says "total" and holds no digits.
fn is_summary_row(row: &Row) -> bool {
    row.get("date").is_some_and(|d| {
        d.to_lowercase().contains("total") && !d.chars().any(|c| c.is_ascii_digit())
    })
}

fn millionths(v: f64) -> i64 {
    (v * 1_000_000.0).round() as i64
}

/// Whether one more of `print` is on the books than this file has matched
/// so far - and if so, counts it matched. Counting rather than a yes/no
/// keeps two genuinely identical trades on one day: a file with both, over
/// a database holding one, imports the second.
fn already_on_file<K: std::hash::Hash + Eq>(prints: &mut HashMap<K, usize>, print: &K) -> bool {
    match prints.get_mut(print) {
        Some(n) if *n > 0 => {
            *n -= 1;
            true
        }
        _ => false,
    }
}
#[derive(Clone, Copy, PartialEq)]
enum Market {
    India,
    Global,
}

fn check_required_columns(spec: &ImportSpec, sheet: &Sheet) -> AppResult<()> {
    let present: HashSet<&str> = sheet.headers.iter().filter_map(|h| spec.canonical(h)).collect();
    let missing: Vec<&str> = spec
        .required_columns()
        .filter(|c| !present.contains(c.name))
        .map(|c| c.heading)
        .collect();

    if missing.is_empty() {
        Ok(())
    } else {
        Err(AppError::bad(format!(
            "the file is missing required column(s): {}. Download the {} template to see the expected format.",
            missing.join(", "),
            spec.title
        )))
    }
}

// ---------------------------------------------------------------------------
// Reference resolution
// ---------------------------------------------------------------------------

/// How an account named in a file is matched to one on the books: by name,
/// ignoring case, among accounts of a compatible kind. A bank account and
/// a deposit account at the same bank are often both just "HDFC", and a
/// balance filed under the deposit account never shows under Cash.
/// The investment kinds match one another, so funds bought through a demat
/// account can name it.
fn account_key(kind: &str, name: &str) -> (&'static str, String) {
    let family = if kind == AccountKind::Bank.as_str() {
        "bank"
    } else if kind == AccountKind::Deposit.as_str() {
        "deposit"
    } else {
        "investment"
    };
    (family, name.to_lowercase())
}

/// Caches the lookups every row needs, and creates accounts and instruments
/// on first mention so a fresh install can be populated from spreadsheets
/// alone.
struct Resolver {
    /// Whose records these are.
    owner: i32,
    /// (kind family, lowercased name) -> account. See [`account_key`].
    accounts: HashMap<(&'static str, String), account::Model>,
    exchanges: HashMap<String, exchange::Model>,
    /// (UPPER symbol, exchange id) -> instrument
    instruments: HashMap<(String, Option<i32>), instrument::Model>,
    by_isin: HashMap<String, instrument::Model>,
    by_amfi: HashMap<String, instrument::Model>,
    /// (account id, reference) pairs already on the books.
    seen_refs: HashSet<(i32, String)>,
    seen_deposit_refs: HashSet<(i32, String)>,
    /// Transactions and deposits on the books by content, with how many of
    /// each this file has not yet matched. How a row with no reference is
    /// recognised as imported before.
    txn_prints: HashMap<TxnPrint, usize>,
    deposit_prints: HashMap<DepositPrint, usize>,
    created_accounts: Vec<String>,
    created_instruments: Vec<String>,
}

impl Resolver {
    async fn load(db: &DatabaseConnection, owner: i32) -> AppResult<Self> {
        let owned = account::Entity::find().filter(account::Column::UserId.eq(owner)).all(db).await?;
        let ids: Vec<i32> = owned.iter().map(|a| a.id).collect();
        let accounts = owned
            .into_iter()
            .map(|a| (account_key(&a.kind, &a.name), a))
            .collect();

        let exchanges = exchange::Entity::find()
            .all(db)
            .await?
            .into_iter()
            .map(|e| (e.code.to_uppercase(), e))
            .collect();

        let all_instruments = instrument::Entity::find().all(db).await?;
        let mut instruments = HashMap::new();
        let mut by_isin = HashMap::new();
        let mut by_amfi = HashMap::new();
        for i in all_instruments {
            if let Some(isin) = &i.isin {
                by_isin.insert(isin.to_uppercase(), i.clone());
            }
            if let Some(code) = &i.amfi_scheme_code {
                by_amfi.insert(code.clone(), i.clone());
            }
            instruments.insert((i.symbol.to_uppercase(), i.exchange_id), i);
        }

        let mut seen_refs = HashSet::new();
        let mut txn_prints = HashMap::new();
        for t in txn::Entity::find().filter(txn::Column::AccountId.is_in(ids.clone())).all(db).await? {
            *txn_prints
                .entry(TxnPrint::new(t.account_id, t.instrument_id, t.trade_date, &t.kind, t.quantity, t.price))
                .or_insert(0) += 1;
            if let Some(r) = t.external_id {
                seen_refs.insert((t.account_id, r));
            }
        }
        let mut seen_deposit_refs = HashSet::new();
        let mut deposit_prints = HashMap::new();
        for d in deposit::Entity::find().filter(deposit::Column::AccountId.is_in(ids)).all(db).await? {
            let print = DepositPrint::new(d.account_id, &d.kind, d.principal, d.interest_rate, d.start_date, d.maturity_date);
            *deposit_prints.entry(print).or_insert(0) += 1;
            if let Some(r) = d.reference {
                seen_deposit_refs.insert((d.account_id, r));
            }
        }

        Ok(Self {
            owner,
            accounts,
            exchanges,
            instruments,
            by_isin,
            by_amfi,
            seen_refs,
            seen_deposit_refs,
            txn_prints,
            deposit_prints,
            created_accounts: Vec::new(),
            created_instruments: Vec::new(),
        })
    }

    async fn account(
        &mut self,
        db: &DatabaseConnection,
        name: &str,
        kind: AccountKind,
        country: &str,
        currency: &str,
        institution: Option<&str>,
        dry_run: bool,
    ) -> AppResult<account::Model> {
        let key = account_key(kind.as_str(), name);
        if let Some(a) = self.accounts.get(&key) {
            return Ok(a.clone());
        }

        let now = chrono::Utc::now().naive_utc();
        let model = account::Model {
            id: 0,
            user_id: self.owner,
            name: name.to_string(),
            kind: kind.to_string(),
            country: country.to_uppercase(),
            currency_code: currency.to_uppercase(),
            institution: institution.map(|s| s.to_string()),
            account_ref: None,
            opened_on: None,
            is_active: true,
            notes: Some("Created automatically by spreadsheet import".to_string()),
            created_at: now,
            updated_at: now,
        };

        let saved = if dry_run {
            model
        } else {
            account::ActiveModel {
                user_id: Set(self.owner),
                name: Set(model.name.clone()),
                kind: Set(model.kind.clone()),
                country: Set(model.country.clone()),
                currency_code: Set(model.currency_code.clone()),
                institution: Set(model.institution.clone()),
                account_ref: Set(None),
                opened_on: Set(None),
                is_active: Set(true),
                notes: Set(model.notes.clone()),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(db)
            .await?
        };

        self.created_accounts.push(saved.name.clone());
        self.accounts.insert(key, saved.clone());
        Ok(saved)
    }

    fn exchange(&self, code: &str) -> Option<&exchange::Model> {
        self.exchanges.get(&code.to_uppercase())
    }

    #[allow(clippy::too_many_arguments)]
    async fn instrument(
        &mut self,
        db: &DatabaseConnection,
        spec: NewInstrument<'_>,
        dry_run: bool,
    ) -> AppResult<instrument::Model> {
        // Identify by AMFI code, then ISIN, then (symbol, exchange). The
        // first two are globally unique; the last is only unique per venue.
        if let Some(code) = spec.amfi_scheme_code {
            if let Some(i) = self.by_amfi.get(code) {
                return Ok(i.clone());
            }
        }
        if let Some(isin) = spec.isin {
            if let Some(i) = self.by_isin.get(&isin.to_uppercase()) {
                return Ok(i.clone());
            }
        }
        let key = (spec.symbol.to_uppercase(), spec.exchange_id);
        if let Some(i) = self.instruments.get(&key) {
            return Ok(i.clone());
        }

        let now = chrono::Utc::now().naive_utc();
        let model = instrument::Model {
            id: 0,
            kind: spec.kind.to_string(),
            symbol: spec.symbol.to_string(),
            name: spec.name.unwrap_or(spec.symbol).to_string(),
            isin: spec.isin.map(|s| s.to_uppercase()),
            exchange_id: spec.exchange_id,
            currency_code: spec.currency.to_uppercase(),
            country: spec.country.map(|s| s.to_uppercase()),
            quote_symbol: spec.quote_symbol.map(|s| s.to_string()),
            amfi_scheme_code: spec.amfi_scheme_code.map(|s| s.to_string()),
            sector: spec.sector.map(|s| s.to_string()),
            history_from: None,
            holdings_ref: None,
            is_active: true,
            created_at: now,
            updated_at: now,
        };

        let saved = if dry_run {
            model
        } else {
            instrument::ActiveModel {
                kind: Set(model.kind.clone()),
                symbol: Set(model.symbol.clone()),
                name: Set(model.name.clone()),
                isin: Set(model.isin.clone()),
                exchange_id: Set(model.exchange_id),
                currency_code: Set(model.currency_code.clone()),
                country: Set(model.country.clone()),
                quote_symbol: Set(model.quote_symbol.clone()),
                amfi_scheme_code: Set(model.amfi_scheme_code.clone()),
                sector: Set(model.sector.clone()),
                is_active: Set(true),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(db)
            .await?
        };

        self.created_instruments.push(format!("{} ({})", saved.name, saved.symbol));
        if let Some(isin) = &saved.isin {
            self.by_isin.insert(isin.clone(), saved.clone());
        }
        if let Some(code) = &saved.amfi_scheme_code {
            self.by_amfi.insert(code.clone(), saved.clone());
        }
        self.instruments
            .insert((saved.symbol.to_uppercase(), saved.exchange_id), saved.clone());
        Ok(saved)
    }
}

struct NewInstrument<'a> {
    kind: AssetKind,
    symbol: &'a str,
    name: Option<&'a str>,
    isin: Option<&'a str>,
    exchange_id: Option<i32>,
    currency: &'a str,
    country: Option<&'a str>,
    quote_symbol: Option<&'a str>,
    amfi_scheme_code: Option<&'a str>,
    sector: Option<&'a str>,
}

// ---------------------------------------------------------------------------
// Row handlers
// ---------------------------------------------------------------------------

async fn apply_security_txn(
    db: &DatabaseConnection,
    r: &mut Resolver,
    row: &Row,
    batch_id: Option<i32>,
    market: Market,
    dry_run: bool,
) -> AppResult<RowResult> {
    let date = reader::parse_date(require(row, "date")?)?;
    let kind = parse_txn_kind(require(row, "type")?)?;
    let symbol = require(row, "symbol")?.to_string();

    let (exchange_code, default_currency, account_kind) = match market {
        Market::India => (row.get("exchange").unwrap_or("NSE").to_string(), "INR".to_string(), AccountKind::DematIn),
        Market::Global => {
            let code = require(row, "exchange")?.to_string();
            let ccy = require(row, "currency")?.to_uppercase();
            (code, ccy, AccountKind::BrokerGlobal)
        }
    };

    let exch = r
        .exchange(&exchange_code)
        .ok_or_else(|| AppError::bad(format!("unknown exchange '{exchange_code}'")))?
        .clone();

    let currency = row
        .get("currency")
        .map(|c| c.to_uppercase())
        .unwrap_or(default_currency);

    let account = r
        .account(
            db,
            require(row, "account")?,
            account_kind,
            &exch.country,
            &currency,
            None,
            dry_run,
        )
        .await?;

    // A reference already on file means this row was imported before.
    let reference = row.get("reference").map(|s| s.to_string());
    let known_ref = reference.as_ref().is_some_and(|id| r.seen_refs.contains(&(account.id, id.clone())));

    let asset_kind = match row.get("asset_kind") {
        Some(k) => AssetKind::from_str(k).map_err(AppError::bad)?,
        None => AssetKind::Equity,
    };

    let instrument = r
        .instrument(
            db,
            NewInstrument {
                kind: asset_kind,
                symbol: &symbol,
                name: row.get("name"),
                isin: row.get("isin"),
                exchange_id: Some(exch.id),
                currency: &currency,
                country: Some(&exch.country),
                quote_symbol: None,
                amfi_scheme_code: None,
                sector: None,
            },
            dry_run,
        )
        .await?;

    let quantity = reader::parse_number(require(row, "quantity")?)?;
    let price = reader::parse_number(require(row, "price")?)?;
    let fees = reader::parse_number_opt(row.get("fees"))?.unwrap_or(0.0);
    let taxes = reader::parse_number_opt(row.get("taxes"))?.unwrap_or(0.0);
    let fx_rate = reader::parse_number_opt(row.get("fx_rate"))?;

    validate_txn(kind, quantity, price)?;

    if seen_before(&mut r.txn_prints, TxnPrint::new(account.id, instrument.id, date, kind.as_str(), quantity, price), known_ref) {
        return Ok(RowResult::Skipped);
    }

    if dry_run {
        return Ok(RowResult::Applied);
    }

    insert_txn(
        db,
        TxnInput {
            account_id: account.id,
            instrument_id: instrument.id,
            trade_date: date,
            kind,
            quantity,
            price,
            fees,
            taxes,
            currency: &currency,
            fx_rate,
            reference: reference.as_deref(),
            batch_id,
            notes: row.get("notes"),
            explicit_amount: None,
        },
    )
    .await?;

    if let Some(ref_id) = reference {
        r.seen_refs.insert((account.id, ref_id));
    }
    Ok(RowResult::Applied)
}

async fn apply_mf_txn(
    db: &DatabaseConnection,
    r: &mut Resolver,
    row: &Row,
    batch_id: Option<i32>,
    dry_run: bool,
) -> AppResult<RowResult> {
    let date = reader::parse_date(require(row, "date")?)?;
    let kind = parse_txn_kind(require(row, "type")?)?;

    let scheme_code = row.get("scheme_code");
    let isin = row.get("isin");
    let scheme_name = row.get("scheme_name");
    if scheme_code.is_none() && isin.is_none() && scheme_name.is_none() {
        return Err(AppError::bad(
            "a fund row needs at least one of AMFI Scheme Code, ISIN or Scheme Name",
        ));
    }

    let account = r
        .account(db, require(row, "account")?, AccountKind::MfFolioIn, "IN", "INR", None, dry_run)
        .await?;

    // A reference already on file means this row was imported before.
    let reference = row.get("reference").map(|s| s.to_string());
    let known_ref = reference.as_ref().is_some_and(|id| r.seen_refs.contains(&(account.id, id.clone())));

    // Funds hang off the AMFI pseudo-exchange so they group cleanly in the UI.
    let amfi_exchange_id = r.exchange("AMFI").map(|e| e.id);
    // The symbol is the AMFI code when we have one - it is the stable key.
    let symbol = scheme_code
        .map(|s| s.to_string())
        .or_else(|| isin.map(|s| s.to_string()))
        .or_else(|| scheme_name.map(|s| s.to_string()))
        .unwrap_or_default();

    let instrument = r
        .instrument(
            db,
            NewInstrument {
                kind: AssetKind::MutualFund,
                symbol: &symbol,
                name: scheme_name,
                isin,
                exchange_id: amfi_exchange_id,
                currency: "INR",
                country: Some("IN"),
                quote_symbol: None,
                amfi_scheme_code: scheme_code,
                sector: None,
            },
            dry_run,
        )
        .await?;

    let nav = reader::parse_number(require(row, "nav")?)?;
    let amount = reader::parse_number_opt(row.get("amount"))?;

    // Registrar statements often give amount but not units, or vice versa.
    let units = match reader::parse_number_opt(row.get("units"))? {
        Some(u) => u,
        None => match amount {
            Some(a) if nav.abs() > f64::EPSILON => a / nav,
            _ => {
                return Err(AppError::bad(
                    "Units is blank and cannot be derived - supply Units, or Amount with a non-zero NAV",
                ))
            }
        },
    };
    // Some exports (Value Research's transaction history among them) sign
    // Units and Amount negative for redemptions/switch-outs instead of
    // relying on Type for direction. The Type column already carries the
    // direction here, so only the magnitude matters.
    let units = units.abs();

    let fees = reader::parse_number_opt(row.get("fees"))?.unwrap_or(0.0);
    let taxes = reader::parse_number_opt(row.get("taxes"))?.unwrap_or(0.0);

    validate_txn(kind, units, nav)?;

    if seen_before(&mut r.txn_prints, TxnPrint::new(account.id, instrument.id, date, kind.as_str(), units, nav), known_ref) {
        return Ok(RowResult::Skipped);
    }

    if dry_run {
        return Ok(RowResult::Applied);
    }

    insert_txn(
        db,
        TxnInput {
            account_id: account.id,
            instrument_id: instrument.id,
            trade_date: date,
            kind,
            quantity: units,
            price: nav,
            fees,
            taxes,
            currency: "INR",
            fx_rate: None,
            reference: reference.as_deref(),
            batch_id,
            notes: row.get("notes"),
            explicit_amount: amount,
        },
    )
    .await?;

    // The statement's own NAV for this date is real history - capture it so
    // performance-over-period and XIRR have more than "since first refresh"
    // to work with, even before any AMFI/mfapi backfill runs.
    refresh::upsert_price(db, instrument.id, date, nav, "INR", "import").await?;

    if let Some(ref_id) = reference {
        r.seen_refs.insert((account.id, ref_id));
    }
    Ok(RowResult::Applied)
}

async fn apply_deposit(
    db: &DatabaseConnection,
    r: &mut Resolver,
    row: &Row,
    dry_run: bool,
) -> AppResult<RowResult> {
    let currency = require(row, "currency")?.to_uppercase();
    let country = row
        .get("country")
        .map(|c| c.to_uppercase())
        .unwrap_or_else(|| country_for_currency(&currency).to_string());

    let institution = require(row, "institution")?;
    let account = r
        .account(
            db,
            require(row, "account")?,
            AccountKind::Deposit,
            &country,
            &currency,
            Some(institution),
            dry_run,
        )
        .await?;

    // A reference already on file means this deposit was imported before.
    let reference = row.get("reference").map(|s| s.to_string());
    let known_ref =
        reference.as_ref().is_some_and(|id| r.seen_deposit_refs.contains(&(account.id, id.clone())));

    let kind = match row.get("kind") {
        Some(k) => DepositKind::from_str(k).map_err(AppError::bad)?,
        None => DepositKind::FixedDeposit,
    };
    let compounding = match row.get("compounding") {
        Some(c) => Compounding::from_str(c).map_err(AppError::bad)?,
        None => Compounding::Quarterly,
    };
    let payout = match row.get("payout") {
        Some(p) => PayoutFrequency::from_str(p).map_err(AppError::bad)?,
        None => PayoutFrequency::Cumulative,
    };

    let principal = reader::parse_number(require(row, "principal")?)?;
    let installment = reader::parse_number_opt(row.get("installment"))?;
    let rate = reader::parse_number(require(row, "interest_rate")?)?;
    let start = reader::parse_date(require(row, "start_date")?)?;
    let maturity = reader::parse_date(require(row, "maturity_date")?)?;

    if maturity <= start {
        return Err(AppError::bad("Maturity Date must be after Start Date"));
    }
    if kind == DepositKind::RecurringDeposit && installment.unwrap_or(0.0) <= 0.0 {
        return Err(AppError::bad("a RECURRING_DEPOSIT needs an Instalment amount"));
    }
    if kind != DepositKind::RecurringDeposit && principal <= 0.0 {
        return Err(AppError::bad("Principal must be greater than zero"));
    }
    if !(0.0..=100.0).contains(&rate) {
        return Err(AppError::bad(format!("Interest Rate {rate} is outside 0-100%")));
    }
    let tds = reader::parse_number_opt(row.get("tds_pct"))?;
    if let Some(t) = tds.filter(|t| !(0.0..=100.0).contains(t)) {
        return Err(AppError::bad(format!("TDS {t} is outside 0-100%")));
    }

    if seen_before(&mut r.deposit_prints, DepositPrint::new(account.id, kind.as_str(), principal, rate, start, maturity), known_ref) {
        return Ok(RowResult::Skipped);
    }

    if dry_run {
        return Ok(RowResult::Applied);
    }

    let now = chrono::Utc::now().naive_utc();
    deposit::ActiveModel {
        account_id: Set(account.id),
        kind: Set(kind.to_string()),
        institution: Set(institution.to_string()),
        reference: Set(reference.clone()),
        principal: Set(principal),
        interest_rate: Set(rate),
        compounding: Set(compounding.to_string()),
        payout_frequency: Set(payout.to_string()),
        installment: Set(installment),
        start_date: Set(start),
        maturity_date: Set(maturity),
        currency_code: Set(currency),
        country: Set(country),
        auto_renew: Set(row.get("auto_renew").map(reader::parse_bool).unwrap_or(false)),
        status: Set("ACTIVE".to_string()),
        maturity_amount: Set(reader::parse_number_opt(row.get("maturity_amount"))?),
        tds_pct: Set(tds),
        notes: Set(row.get("notes").map(|s| s.to_string())),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await?;

    if let Some(ref_id) = reference {
        r.seen_deposit_refs.insert((account.id, ref_id));
    }
    Ok(RowResult::Applied)
}

async fn apply_cash(
    db: &DatabaseConnection,
    r: &mut Resolver,
    row: &Row,
    dry_run: bool,
) -> AppResult<RowResult> {
    let currency = require(row, "currency")?.to_uppercase();
    let country = row
        .get("country")
        .map(|c| c.to_uppercase())
        .unwrap_or_else(|| country_for_currency(&currency).to_string());

    let account = r
        .account(
            db,
            require(row, "account")?,
            AccountKind::Bank,
            &country,
            &currency,
            row.get("institution"),
            dry_run,
        )
        .await?;

    let as_of = reader::parse_date(require(row, "as_of")?)?;
    let balance = reader::parse_number(require(row, "balance")?)?;

    // One snapshot per account per day - a re-upload corrects rather than
    // stacks up duplicates, and one that changes nothing is already on file.
    let existing = cash_balance::Entity::find()
        .filter(cash_balance::Column::AccountId.eq(account.id))
        .filter(cash_balance::Column::AsOf.eq(as_of))
        .one(db)
        .await?;
    if existing
        .as_ref()
        .is_some_and(|e| millionths(e.balance) == millionths(balance) && e.currency_code == currency)
    {
        return Ok(RowResult::Skipped);
    }

    if dry_run {
        return Ok(RowResult::Applied);
    }

    match existing {
        Some(found) => {
            let mut m: cash_balance::ActiveModel = found.into();
            m.balance = Set(balance);
            m.currency_code = Set(currency);
            m.source = Set("import".to_string());
            m.notes = Set(row.get("notes").map(|s| s.to_string()));
            m.update(db).await?;
        }
        None => {
            cash_balance::ActiveModel {
                account_id: Set(account.id),
                as_of: Set(as_of),
                balance: Set(balance),
                currency_code: Set(currency),
                source: Set("import".to_string()),
                notes: Set(row.get("notes").map(|s| s.to_string())),
                created_at: Set(chrono::Utc::now().naive_utc()),
                ..Default::default()
            }
            .insert(db)
            .await?;
        }
    }

    Ok(RowResult::Applied)
}

async fn apply_instrument(
    db: &DatabaseConnection,
    r: &mut Resolver,
    row: &Row,
    dry_run: bool,
) -> AppResult<RowResult> {
    let symbol = require(row, "symbol")?;
    let kind = AssetKind::from_str(require(row, "kind")?).map_err(AppError::bad)?;

    let exchange = row.get("exchange").and_then(|c| r.exchange(c)).cloned();
    let currency = row
        .get("currency")
        .map(|c| c.to_uppercase())
        .or_else(|| exchange.as_ref().map(|e| e.currency_code.clone()))
        .ok_or_else(|| AppError::bad("Currency is required when Exchange is blank or unknown"))?;
    let country = row
        .get("country")
        .map(|c| c.to_uppercase())
        .or_else(|| exchange.as_ref().map(|e| e.country.clone()));

    r.instrument(
        db,
        NewInstrument {
            kind,
            symbol,
            name: row.get("name"),
            isin: row.get("isin"),
            exchange_id: exchange.as_ref().map(|e| e.id),
            currency: &currency,
            country: country.as_deref(),
            quote_symbol: row.get("quote_symbol"),
            amfi_scheme_code: row.get("amfi_scheme_code"),
            sector: row.get("sector"),
        },
        dry_run,
    )
    .await?;

    Ok(RowResult::Applied)
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

struct TxnInput<'a> {
    account_id: i32,
    instrument_id: i32,
    trade_date: chrono::NaiveDate,
    kind: TxnKind,
    quantity: f64,
    price: f64,
    fees: f64,
    taxes: f64,
    currency: &'a str,
    fx_rate: Option<f64>,
    reference: Option<&'a str>,
    batch_id: Option<i32>,
    notes: Option<&'a str>,
    /// Gross amount straight from the statement, when supplied.
    explicit_amount: Option<f64>,
}

async fn insert_txn(db: &DatabaseConnection, input: TxnInput<'_>) -> AppResult<()> {
    txn::ActiveModel {
        account_id: Set(input.account_id),
        instrument_id: Set(input.instrument_id),
        trade_date: Set(input.trade_date),
        kind: Set(input.kind.to_string()),
        quantity: Set(input.quantity.abs()),
        price: Set(input.price),
        fees: Set(input.fees.abs()),
        taxes: Set(input.taxes.abs()),
        net_amount: Set(net_amount(&input)),
        currency_code: Set(input.currency.to_uppercase()),
        fx_rate: Set(input.fx_rate),
        external_id: Set(input.reference.map(|s| s.to_string())),
        import_batch_id: Set(input.batch_id),
        notes: Set(input.notes.map(|s| s.to_string())),
        created_at: Set(chrono::Utc::now().naive_utc()),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(())
}

/// Signed cash impact: negative when money leaves, positive when it arrives.
fn net_amount(input: &TxnInput<'_>) -> f64 {
    let gross = input
        .explicit_amount
        .map(f64::abs)
        .unwrap_or_else(|| input.quantity.abs() * input.price);
    match input.kind {
        TxnKind::Buy | TxnKind::Reinvest | TxnKind::TransferIn => {
            -(gross + input.fees.abs() + input.taxes.abs())
        }
        TxnKind::Sell | TxnKind::TransferOut | TxnKind::Dividend => {
            gross - input.fees.abs() - input.taxes.abs()
        }
        // No money changes hands.
        TxnKind::Bonus | TxnKind::Split => 0.0,
    }
}

fn validate_txn(kind: TxnKind, quantity: f64, price: f64) -> AppResult<()> {
    match kind {
        TxnKind::Split => {
            if quantity <= 0.0 {
                return Err(AppError::bad(
                    "a SPLIT row needs a positive ratio in Quantity (2 means 1-for-2)",
                ));
            }
        }
        TxnKind::Bonus => {
            if quantity <= 0.0 {
                return Err(AppError::bad("a BONUS row needs a positive Quantity"));
            }
        }
        _ => {
            if quantity <= 0.0 {
                return Err(AppError::bad("Quantity must be greater than zero"));
            }
            if price < 0.0 {
                return Err(AppError::bad("Price cannot be negative"));
            }
        }
    }
    Ok(())
}

/// Accepts the wording brokers and registrars actually use.
pub fn parse_txn_kind(raw: &str) -> AppResult<TxnKind> {
    let k = raw.trim().to_ascii_uppercase().replace([' ', '-', '/'], "_");
    let mapped = match k.as_str() {
        "B" | "BUY" | "PURCHASE" | "PURCHASES" | "SIP" | "SUBSCRIPTION" | "CREDIT"
        | "INVESTMENT" | "INVESTMENT_IN_FUND" => "BUY",
        "S" | "SELL" | "SALE" | "REDEMPTION" | "REDEEM" | "REDEMPTIONS" | "DEBIT"
        | "SELL_REDEMPTION" => "SELL",
        "DIV" | "DIVIDEND" | "IDCW" | "DIVIDEND_PAYOUT" | "DISTRIBUTION" => "DIVIDEND",
        "REINVEST" | "DIVIDEND_REINVEST" | "IDCW_REINVEST" | "REINVESTMENT" => "REINVEST",
        "BONUS" | "BONUS_ISSUE" => "BONUS",
        "SPLIT" | "STOCK_SPLIT" | "SUBDIVISION" => "SPLIT",
        // Value Research's "Merger" rows are a fund-house-forced switch: units
        // leave the merged scheme and land in the surviving one on the same day.
        "SWITCH_IN" | "TRANSFER_IN" | "IN" | "SWITCH_INVESTMENT" | "MERGER_INVESTMENT" => {
            "TRANSFER_IN"
        }
        "SWITCH_OUT" | "TRANSFER_OUT" | "OUT" | "SWITCH_REDEMPTION" | "MERGER_REDEMPTION" => {
            "TRANSFER_OUT"
        }
        other => other,
    };
    TxnKind::from_str(mapped).map_err(|_| {
        AppError::bad(format!(
            "'{raw}' is not a transaction type we recognise - use BUY, SELL, DIVIDEND, REINVEST, BONUS, SPLIT, TRANSFER_IN or TRANSFER_OUT"
        ))
    })
}

/// Sensible default so a deposit row can omit Country.
pub fn country_for_currency(currency: &str) -> &'static str {
    match currency.to_uppercase().as_str() {
        "INR" => "IN",
        "SGD" => "SG",
        "AUD" => "AU",
        "USD" => "US",
        "GBP" => "GB",
        "EUR" => "EU",
        "HKD" => "HK",
        "JPY" => "JP",
        "CAD" => "CA",
        "CHF" => "CH",
        "AED" => "AE",
        "NZD" => "NZ",
        _ => "??",
    }
}

fn require<'a>(row: &'a Row, key: &str) -> AppResult<&'a str> {
    row.get(key)
        .ok_or_else(|| AppError::bad(format!("'{key}' is required but empty")))
}

// ---------------------------------------------------------------------------
// Batch bookkeeping
// ---------------------------------------------------------------------------

async fn create_batch(
    db: &DatabaseConnection,
    owner: i32,
    kind: ImportKind,
    filename: &str,
    rows: usize,
) -> AppResult<i32> {
    let saved = import_batch::ActiveModel {
        user_id: Set(owner),
        kind: Set(kind.to_string()),
        filename: Set(filename.to_string()),
        status: Set(ImportStatus::Pending.to_string()),
        row_count: Set(rows as i32),
        ok_count: Set(0),
        error_count: Set(0),
        skipped_count: Set(0),
        errors_json: Set(None),
        message: Set(None),
        created_at: Set(chrono::Utc::now().naive_utc()),
        ..Default::default()
    }
    .insert(db)
    .await?;
    Ok(saved.id)
}

async fn finalise_batch(
    db: &DatabaseConnection,
    id: i32,
    status: ImportStatus,
    ok: usize,
    skipped: usize,
    errors: &[RowError],
) -> AppResult<()> {
    let Some(row) = import_batch::Entity::find_by_id(id).one(db).await? else {
        return Ok(());
    };
    let mut m: import_batch::ActiveModel = row.into();
    m.status = Set(status.to_string());
    m.ok_count = Set(ok as i32);
    m.skipped_count = Set(skipped as i32);
    m.error_count = Set(errors.len() as i32);
    m.errors_json = Set(if errors.is_empty() {
        None
    } else {
        serde_json::to_string(errors).ok()
    });
    m.update(db).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day() -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(2026, 9, 3).unwrap()
    }

    #[test]
    fn a_row_matches_what_is_on_file_once_per_copy_held() {
        let print = TxnPrint::new(1, 29, day(), "BUY", 561.666, 168.1199);
        let mut prints = HashMap::from([(print.clone(), 1usize)]);
        // The same figures as parsed again, float noise and a negative sign aside.
        let again = TxnPrint::new(1, 29, day(), "buy", -561.666_000_000_1, 168.119_900_000_01);
        assert_eq!(print, again);
        assert!(already_on_file(&mut prints, &again), "a re-uploaded row is recognised");
        assert!(!already_on_file(&mut prints, &again), "a second identical trade in the file is new");
        let other = TxnPrint::new(1, 29, day(), "BUY", 561.667, 168.1199);
        assert!(!already_on_file(&mut prints, &other));
    }

    #[test]
    fn a_bank_account_is_never_matched_to_a_deposit_account_of_the_same_name() {
        let bank = account_key("BANK", "HDFC");
        assert_ne!(bank, account_key("DEPOSIT", "HDFC"));
        assert_ne!(bank, account_key("DEMAT_IN", "HDFC"));
        assert_eq!(bank, account_key("BANK", "hdfc"));
        // Funds bought through a demat account may name it.
        assert_eq!(account_key("MF_FOLIO_IN", "Zerodha"), account_key("DEMAT_IN", "zerodha"));
    }

    #[test]
    fn totals_lines_are_not_rows() {
        let row = |d: &str| Row { number: 1, cells: HashMap::from([("date".to_string(), d.to_string())]) };
        assert!(is_summary_row(&row("Mutual Funds / SIFs Total")));
        assert!(is_summary_row(&row("TOTAL")));
        assert!(!is_summary_row(&row("03-09-2026")));
        assert!(!is_summary_row(&Row { number: 1, cells: HashMap::new() }));
    }

    #[test]
    fn accepts_the_wording_brokers_and_registrars_use() {
        assert_eq!(parse_txn_kind("Purchase").unwrap(), TxnKind::Buy);
        assert_eq!(parse_txn_kind("SIP").unwrap(), TxnKind::Buy);
        assert_eq!(parse_txn_kind("redemption").unwrap(), TxnKind::Sell);
        assert_eq!(parse_txn_kind("IDCW").unwrap(), TxnKind::Dividend);
        assert_eq!(parse_txn_kind("Switch Out").unwrap(), TxnKind::TransferOut);
        assert_eq!(parse_txn_kind("Stock-Split").unwrap(), TxnKind::Split);
        assert_eq!(parse_txn_kind("B").unwrap(), TxnKind::Buy);
    }

    #[test]
    fn accepts_value_research_online_wording() {
        assert_eq!(parse_txn_kind("Investment in fund").unwrap(), TxnKind::Buy);
        assert_eq!(parse_txn_kind("Sell/Redemption").unwrap(), TxnKind::Sell);
        assert_eq!(parse_txn_kind("Switch Investment").unwrap(), TxnKind::TransferIn);
        assert_eq!(parse_txn_kind("Switch Redemption").unwrap(), TxnKind::TransferOut);
        assert_eq!(parse_txn_kind("Merger Investment").unwrap(), TxnKind::TransferIn);
        assert_eq!(parse_txn_kind("Merger Redemption").unwrap(), TxnKind::TransferOut);
    }

    #[test]
    fn rejects_unknown_types_with_the_valid_list() {
        let err = parse_txn_kind("hodl").unwrap_err().to_string();
        assert!(err.contains("BUY"), "got: {err}");
    }

    #[test]
    fn infers_country_from_currency() {
        assert_eq!(country_for_currency("inr"), "IN");
        assert_eq!(country_for_currency("SGD"), "SG");
        assert_eq!(country_for_currency("AUD"), "AU");
        assert_eq!(country_for_currency("XYZ"), "??");
    }

    fn input(kind: TxnKind, qty: f64, price: f64, fees: f64, taxes: f64) -> TxnInput<'static> {
        TxnInput {
            account_id: 1,
            instrument_id: 1,
            trade_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            kind,
            quantity: qty,
            price,
            fees,
            taxes,
            currency: "INR",
            fx_rate: None,
            reference: None,
            batch_id: None,
            notes: None,
            explicit_amount: None,
        }
    }

    #[test]
    fn a_buy_is_money_out_including_costs() {
        assert_eq!(net_amount(&input(TxnKind::Buy, 10.0, 100.0, 20.0, 5.0)), -1025.0);
    }

    #[test]
    fn a_sell_is_money_in_net_of_costs() {
        assert_eq!(net_amount(&input(TxnKind::Sell, 10.0, 100.0, 20.0, 5.0)), 975.0);
    }

    #[test]
    fn corporate_actions_move_no_cash() {
        assert_eq!(net_amount(&input(TxnKind::Bonus, 10.0, 0.0, 0.0, 0.0)), 0.0);
        assert_eq!(net_amount(&input(TxnKind::Split, 2.0, 0.0, 0.0, 0.0)), 0.0);
    }

    #[test]
    fn an_explicit_statement_amount_wins_over_units_times_price() {
        let mut i = input(TxnKind::Buy, 32.456, 1543.21, 0.0, 2.5);
        i.explicit_amount = Some(50_000.0);
        // Rounded units would give 50,086; the statement says 50,000.
        assert_eq!(net_amount(&i), -50_002.5);
    }

    #[test]
    fn validation_catches_the_common_spreadsheet_mistakes() {
        assert!(validate_txn(TxnKind::Buy, 0.0, 100.0).is_err());
        assert!(validate_txn(TxnKind::Buy, -5.0, 100.0).is_err());
        assert!(validate_txn(TxnKind::Buy, 5.0, -1.0).is_err());
        assert!(validate_txn(TxnKind::Split, 0.0, 0.0).is_err());
        assert!(validate_txn(TxnKind::Split, 2.0, 0.0).is_ok());
        assert!(validate_txn(TxnKind::Bonus, 10.0, 0.0).is_ok());
    }
}
