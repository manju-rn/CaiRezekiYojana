//! The one import CaiRezekiYojana has: a single CSV holding every position -
//! stocks, ETFs, mutual funds, deposits and cash - as it stands today.
//!
//! The planner needs what is held, not how it was bought, so there is no
//! transaction history here: each row is one holding. Underneath, the rows
//! are sorted into the five formats the importer already knows (Indian
//! stocks, Indian funds, global stocks and ETFs, deposits, cash balances)
//! and each is applied by the existing code, so every holding goes through
//! the same matching, validation and duplicate checks as before. A share or
//! fund becomes a single BUY dated as given, at the price given.
//!
//! The split keeps each row at its row number in the uploaded file - rows
//! belonging to another format are left as empty lines - so an error always
//! points at the line the person can find in their spreadsheet.

use std::collections::{BTreeMap, HashSet};

use chrono::NaiveDate;
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};
use crate::importer::apply::RowError;
use crate::importer::reader::{self, normalise_header, parse_number};
use crate::importer::specs::{spec_for, ColumnSpec, ImportSpec};
use crate::types::ImportKind;

pub const KIND: &str = "PORTFOLIO";

const NOTES: &[&str] = &[
    "One row per holding. The first row must be the column headings; heading matching ignores case, spaces and punctuation.",
    "Category says what the row is: STOCK, ETF, REIT, MUTUAL_FUND, DEPOSIT or CASH.",
    "STOCK, ETF and REIT rows need Symbol, Exchange (SGX, LSE, NASDAQ, NYSE, NSE ...), Quantity, Price and Currency. NSE and BSE rows are Indian stocks; any other exchange is a global listing.",
    "MUTUAL_FUND rows are Indian (AMFI) schemes: Name (or AMFI Scheme Code), Quantity as units and Price as NAV - or Amount, with either. Currency defaults to INR.",
    "DEPOSIT rows need Institution, Amount (the principal), Interest Rate %, Date (the start), Maturity Date and Currency.",
    "CASH rows need Amount (the balance) and Currency; Institution is the bank.",
    "Date is when the holding was bought, or the balance recorded; blank means today. It does not change what the holding is worth today.",
    "An import adds to what is already held. To replace the portfolio, delete it first (Settings > Delete data) and import again.",
    "Prices, NAVs and exchange rates are refreshed from the feeds afterwards, so today's value moves with the market.",
];

pub static PORTFOLIO: ImportSpec = ImportSpec {
    kind: KIND,
    title: "Portfolio - every holding in one file",
    description: "Stocks, ETFs, mutual funds, deposits and cash balances in a single CSV, one row each, for the planner to work from.",
    columns: &[
        ColumnSpec { name: "category", heading: "Category", aliases: &["asset_class", "asset_type", "holding_type", "kind"], required: true, example: "ETF", description: "STOCK, ETF, REIT, MUTUAL_FUND, DEPOSIT or CASH." },
        ColumnSpec { name: "account", heading: "Account", aliases: &["broker", "account_name", "portfolio", "folio", "platform"], required: true, example: "IBKR", description: "Brokerage, folio, bank or deposit account. Created on first sight." },
        ColumnSpec { name: "symbol", heading: "Symbol", aliases: &["ticker", "code", "security"], required: false, example: "G3B", description: "Ticker as listed on the exchange, without a suffix. Not needed for funds, deposits or cash." },
        ColumnSpec { name: "exchange", heading: "Exchange", aliases: &["exch", "market", "listing_exchange"], required: false, example: "SGX", description: "SGX, LSE, NASDAQ, NYSE, NYSEARCA, ASX, HKEX, NSE, BSE ... Needed for STOCK, ETF and REIT rows." },
        ColumnSpec { name: "name", heading: "Name", aliases: &["security_name", "scheme_name", "fund", "fund_name", "description"], required: false, example: "Nikko AM Singapore STI ETF", description: "Name of the security or fund. A fund is matched by it when there is no scheme code." },
        ColumnSpec { name: "isin", heading: "ISIN", aliases: &[], required: false, example: "SG1YA3000008", description: "Optional identifier." },
        ColumnSpec { name: "scheme_code", heading: "AMFI Scheme Code", aliases: &["amfi_scheme_code", "amfi_code"], required: false, example: "", description: "Mutual funds only. How the NAV is matched when you have it." },
        ColumnSpec { name: "quantity", heading: "Quantity", aliases: &["qty", "shares", "units", "no_of_units"], required: false, example: "5000", description: "Shares or fund units held." },
        ColumnSpec { name: "price", heading: "Price", aliases: &["nav", "unit_price", "rate", "avg_price", "cost_price"], required: false, example: "3.95", description: "Price per share, or NAV per unit, in the row's Currency." },
        ColumnSpec { name: "amount", heading: "Amount", aliases: &["value", "balance", "principal", "invested"], required: false, example: "", description: "Cash balance, deposit principal, or a fund's value - with Quantity or Price it fills in the other." },
        ColumnSpec { name: "currency", heading: "Currency", aliases: &["ccy", "curr", "trade_currency"], required: true, example: "SGD", description: "ISO code the price or balance is in: SGD, USD, GBP, INR ..." },
        ColumnSpec { name: "country", heading: "Country", aliases: &["country_code"], required: false, example: "SG", description: "Optional; inferred from the currency when blank." },
        ColumnSpec { name: "date", heading: "Date", aliases: &["as_of", "purchase_date", "start_date", "trade_date"], required: false, example: "", description: "When it was bought or recorded, or a deposit's start. Blank means today." },
        ColumnSpec { name: "institution", heading: "Institution", aliases: &["bank", "bank_name", "issuer"], required: false, example: "", description: "Deposits and cash: the bank." },
        ColumnSpec { name: "deposit_kind", heading: "Deposit Kind", aliases: &["deposit_type", "product"], required: false, example: "", description: "FIXED_DEPOSIT (default), RECURRING_DEPOSIT or SCHEME." },
        ColumnSpec { name: "interest_rate", heading: "Interest Rate %", aliases: &["rate_pa", "roi", "interest"], required: false, example: "", description: "Deposits: annual nominal rate as a percentage." },
        ColumnSpec { name: "maturity_date", heading: "Maturity Date", aliases: &["maturity", "end_date", "matures_on"], required: false, example: "", description: "Deposits: the date it matures." },
        ColumnSpec { name: "compounding", heading: "Compounding", aliases: &["compound"], required: false, example: "", description: "Deposits: SIMPLE, MONTHLY, QUARTERLY, HALF_YEARLY, ANNUAL or AT_MATURITY. Defaults to QUARTERLY." },
        ColumnSpec { name: "payout", heading: "Payout", aliases: &["payout_frequency"], required: false, example: "", description: "Deposits: CUMULATIVE (default) or MONTHLY, QUARTERLY, HALF_YEARLY, ANNUAL." },
        ColumnSpec { name: "notes", heading: "Notes", aliases: &["remarks", "comment"], required: false, example: "", description: "Free text." },
    ],
    sample_rows: &[
        &["ETF", "Tiger SG", "G3B", "SGX", "Nikko AM Singapore STI ETF", "", "", "3000", "4.00", "", "SGD", "SG", "", "", "", "", "", "", "", ""],
        &["ETF", "IBKR", "SWRD", "LSE", "SPDR MSCI World UCITS ETF", "", "", "250", "45.00", "", "USD", "IE", "", "", "", "", "", "", "", ""],
        &["MUTUAL_FUND", "Groww Folio", "", "", "DSP Large Cap Fund - Direct Plan - Growth", "", "", "800", "480.00", "", "INR", "IN", "", "", "", "", "", "", "", ""],
        &["DEPOSIT", "DBS Deposits", "", "", "12-month fixed deposit", "", "", "", "", "20000", "SGD", "SG", "2026-05-15", "DBS Bank", "FIXED_DEPOSIT", "3.10", "2027-05-15", "AT_MATURITY", "CUMULATIVE", ""],
        &["CASH", "DBS Multiplier", "", "", "", "", "", "", "", "25000", "SGD", "SG", "", "DBS Bank", "", "", "", "", "", ""],
    ],
    notes: NOTES,
};

/// One of the existing formats' worth of rows, as CSV text.
#[derive(Debug)]
pub struct Part {
    pub kind: ImportKind,
    pub csv: String,
}

/// An uploaded portfolio sorted into the formats the importer applies.
#[derive(Debug)]
pub struct Split {
    pub parts: Vec<Part>,
    /// Rows that could not even be sorted: no recognisable category.
    pub errors: Vec<RowError>,
    /// Rows read from the file (blank lines excluded).
    pub row_count: usize,
    /// Headings in the file that nothing claims.
    pub unrecognised_columns: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Category {
    Stock,
    Etf,
    Reit,
    MutualFund,
    Deposit,
    Cash,
}

fn category(raw: &str) -> Option<Category> {
    match normalise_header(raw).as_str() {
        "stock" | "stocks" | "equity" | "equities" | "share" | "shares" => Some(Category::Stock),
        "etf" | "etfs" => Some(Category::Etf),
        "reit" | "reits" => Some(Category::Reit),
        "mutual_fund" | "mutual_funds" | "mf" | "fund" | "funds" => Some(Category::MutualFund),
        "deposit" | "deposits" | "fd" | "fixed_deposit" | "rd" | "recurring_deposit" | "term_deposit" => {
            Some(Category::Deposit)
        }
        "cash" | "bank" | "bank_balance" | "balance" | "savings" => Some(Category::Cash),
        _ => None,
    }
}

/// Rows of one target format, by the row number they had in the upload.
struct Out {
    kind: ImportKind,
    spec: &'static ImportSpec,
    rows: BTreeMap<usize, Vec<String>>,
}

impl Out {
    fn new(kind: ImportKind) -> Self {
        Out { kind, spec: spec_for(kind), rows: BTreeMap::new() }
    }

    /// Add a row at `number`, giving the values by the target format's column names.
    fn put(&mut self, number: usize, values: &[(&str, String)]) {
        let cells = self
            .spec
            .columns
            .iter()
            .map(|c| values.iter().find(|(n, _)| *n == c.name).map(|(_, v)| v.clone()).unwrap_or_default())
            .collect();
        self.rows.insert(number, cells);
    }

    /// The CSV: the format's own headings, and a line for every row number up
    /// to the last, the ones not ours made of separators alone (the reader
    /// skips a row with nothing in it but still counts it).
    fn csv(&self) -> Option<String> {
        let last = *self.rows.keys().next_back()?;
        let width = self.spec.columns.len();
        let mut out = String::new();
        let headings: Vec<String> = self.spec.columns.iter().map(|c| escape(c.heading)).collect();
        out.push_str(&headings.join(","));
        out.push('\n');
        for number in 2..=last {
            match self.rows.get(&number) {
                Some(cells) => out.push_str(&cells.iter().map(|c| escape(c)).collect::<Vec<_>>().join(",")),
                None => out.push_str(&",".repeat(width - 1)),
            }
            out.push('\n');
        }
        Some(out)
    }
}

fn escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// A short stable reference for a holding, so the same row uploaded twice is
/// recognised the second time.
fn reference(parts: &[&str]) -> String {
    let digest = Sha256::digest(parts.join("|").to_lowercase().as_bytes());
    let hex: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
    format!("CRY-{hex}")
}

/// A number written for the importer's parser.
fn plain(n: f64) -> String {
    let s = format!("{n:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Sort an uploaded portfolio into the importer's formats.
pub fn split(filename: &str, bytes: &[u8], today: NaiveDate) -> AppResult<Split> {
    let sheet = reader::read(filename, bytes)?;

    let present: HashSet<&str> = sheet.headers.iter().filter_map(|h| PORTFOLIO.canonical(h)).collect();
    let missing: Vec<&str> = PORTFOLIO.required_columns().filter(|c| !present.contains(c.name)).map(|c| c.heading).collect();
    if !missing.is_empty() {
        return Err(AppError::bad(format!(
            "the file is missing required column(s): {}. Download the template to see the expected format.",
            missing.join(", ")
        )));
    }
    let unrecognised_columns: Vec<String> =
        sheet.headers.iter().filter(|h| !h.is_empty() && PORTFOLIO.canonical(h).is_none()).cloned().collect();

    let mut india_equity = Out::new(ImportKind::IndiaEquity);
    let mut india_mf = Out::new(ImportKind::IndiaMf);
    let mut global = Out::new(ImportKind::Global);
    let mut deposits = Out::new(ImportKind::Deposits);
    let mut cash = Out::new(ImportKind::CashBalances);
    let mut errors = Vec::new();
    let today_text = today.format("%Y-%m-%d").to_string();

    for row in &sheet.rows {
        // Re-key the row onto the format's own column names.
        let cells: BTreeMap<&str, &str> = row
            .cells
            .iter()
            .filter_map(|(k, v)| PORTFOLIO.canonical(k).map(|c| (c, v.as_str())))
            .filter(|(_, v)| !v.trim().is_empty())
            .collect();
        let get = |name: &str| cells.get(name).map(|v| v.trim()).unwrap_or("");
        let text = |name: &str| get(name).to_string();

        let Some(cat) = category(get("category")) else {
            errors.push(RowError {
                row: row.number,
                message: format!(
                    "category '{}' is not one of STOCK, ETF, REIT, MUTUAL_FUND, DEPOSIT, CASH",
                    get("category")
                ),
            });
            continue;
        };
        if get("account").is_empty() {
            errors.push(RowError { row: row.number, message: "account is empty".into() });
            continue;
        }
        let date = if get("date").is_empty() { today_text.clone() } else { text("date") };
        let key = |extra: &str| {
            reference(&[
                get("category"),
                get("account"),
                get("symbol"),
                get("exchange"),
                get("name"),
                get("quantity"),
                get("price"),
                get("amount"),
                get("currency"),
                date.as_str(),
                extra,
            ])
        };

        match cat {
            Category::Stock | Category::Etf | Category::Reit => {
                let exchange = get("exchange").to_uppercase();
                let currency = text("currency").to_uppercase();
                let indian = exchange == "NSE" || exchange == "BSE" || (exchange.is_empty() && currency == "INR");
                if indian {
                    india_equity.put(
                        row.number,
                        &[
                            ("date", date.clone()),
                            ("account", text("account")),
                            ("symbol", text("symbol")),
                            ("exchange", if exchange.is_empty() { "NSE".to_string() } else { exchange }),
                            ("name", text("name")),
                            ("isin", text("isin")),
                            ("type", "BUY".to_string()),
                            ("quantity", text("quantity")),
                            ("price", text("price")),
                            ("reference", key("")),
                            ("notes", text("notes")),
                        ],
                    );
                } else {
                    let asset_kind = match cat {
                        Category::Etf => "ETF",
                        Category::Reit => "REIT",
                        _ => "EQUITY",
                    };
                    global.put(
                        row.number,
                        &[
                            ("date", date.clone()),
                            ("account", text("account")),
                            ("symbol", text("symbol")),
                            ("exchange", exchange),
                            ("name", text("name")),
                            ("asset_kind", asset_kind.to_string()),
                            ("isin", text("isin")),
                            ("type", "BUY".to_string()),
                            ("quantity", text("quantity")),
                            ("price", text("price")),
                            ("currency", currency),
                            ("reference", key("")),
                            ("notes", text("notes")),
                        ],
                    );
                }
            }
            Category::MutualFund => {
                // With units and an amount but no NAV, the NAV is what they imply.
                let mut price = text("price");
                if price.is_empty() {
                    if let (Ok(q), Ok(a)) = (parse_number(get("quantity")), parse_number(get("amount"))) {
                        if q > 0.0 {
                            price = plain(a / q);
                        }
                    }
                }
                india_mf.put(
                    row.number,
                    &[
                        ("date", date.clone()),
                        ("account", text("account")),
                        ("scheme_code", text("scheme_code")),
                        ("isin", text("isin")),
                        ("scheme_name", text("name")),
                        ("type", "BUY".to_string()),
                        ("units", text("quantity")),
                        ("nav", price),
                        ("amount", text("amount")),
                        ("reference", key("")),
                        ("notes", text("notes")),
                    ],
                );
            }
            Category::Deposit => {
                deposits.put(
                    row.number,
                    &[
                        ("account", text("account")),
                        ("institution", if get("institution").is_empty() { text("account") } else { text("institution") }),
                        ("kind", text("deposit_kind")),
                        ("reference", key(get("maturity_date"))),
                        ("principal", text("amount")),
                        ("interest_rate", text("interest_rate")),
                        ("compounding", text("compounding")),
                        ("payout", text("payout")),
                        ("start_date", date.clone()),
                        ("maturity_date", text("maturity_date")),
                        ("currency", text("currency").to_uppercase()),
                        ("country", text("country")),
                        ("notes", text("notes")),
                    ],
                );
            }
            Category::Cash => {
                cash.put(
                    row.number,
                    &[
                        ("account", text("account")),
                        ("institution", text("institution")),
                        ("as_of", date.clone()),
                        ("balance", text("amount")),
                        ("currency", text("currency").to_uppercase()),
                        ("country", text("country")),
                        ("notes", text("notes")),
                    ],
                );
            }
        }
    }

    let parts = [india_equity, india_mf, global, deposits, cash]
        .into_iter()
        .filter_map(|o| o.csv().map(|csv| Part { kind: o.kind, csv }))
        .collect();
    Ok(Split { parts, errors, row_count: sheet.rows.len(), unrecognised_columns })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 2).unwrap()
    }

    #[test]
    fn the_template_headings_map_back_to_their_columns() {
        for col in PORTFOLIO.columns {
            assert_eq!(PORTFOLIO.canonical(&normalise_header(col.heading)), Some(col.name), "{}", col.heading);
        }
        for row in PORTFOLIO.sample_rows {
            assert_eq!(row.len(), PORTFOLIO.columns.len());
        }
    }

    #[test]
    fn the_template_splits_into_every_format() {
        let split = split("template.csv", PORTFOLIO.template_csv().as_bytes(), day()).unwrap();
        assert!(split.errors.is_empty(), "{:?}", split.errors);
        let kinds: Vec<ImportKind> = split.parts.iter().map(|p| p.kind).collect();
        assert_eq!(
            kinds,
            vec![ImportKind::IndiaMf, ImportKind::Global, ImportKind::Deposits, ImportKind::CashBalances]
        );
    }

    #[test]
    fn rows_keep_their_line_numbers_in_each_part() {
        let csv = "Category,Account,Currency,Exchange,Symbol,Quantity,Price,Amount\n\
                   ETF,IBKR,USD,LSE,SWRD,10,45,\n\
                   CASH,DBS,SGD,,,,,1000\n\
                   ETF,IBKR,USD,LSE,IGLN,5,60,\n";
        let split = split("p.csv", csv.as_bytes(), day()).unwrap();
        let global = split.parts.iter().find(|p| p.kind == ImportKind::Global).unwrap();
        let lines: Vec<&str> = global.csv.lines().collect();
        // Heading, row 2, an empty line for row 3 (the cash row), row 4.
        assert_eq!(lines.len(), 4);
        assert!(lines[1].contains("SWRD") && lines[3].contains("IGLN"));
        assert!(lines[2].chars().all(|c| c == ','));
        // And the importer's own reader reads it back with the same numbers.
        let sheet = reader::read("global.csv", global.csv.as_bytes()).unwrap();
        let numbers: Vec<usize> = sheet.rows.iter().map(|r| r.number).collect();
        assert_eq!(numbers, vec![2, 4]);
    }

    #[test]
    fn an_unknown_category_is_reported_at_its_row() {
        let csv = "Category,Account,Currency\nBONDS-ish,X,SGD\n";
        let split = split("p.csv", csv.as_bytes(), day()).unwrap();
        assert_eq!(split.errors.len(), 1);
        assert_eq!(split.errors[0].row, 2);
        assert!(split.parts.is_empty());
    }

    #[test]
    fn indian_listings_go_to_indian_stocks_and_the_rest_are_global() {
        let csv = "Category,Account,Currency,Exchange,Symbol,Quantity,Price\n\
                   STOCK,Zerodha,INR,NSE,INFY,5,1500\n\
                   STOCK,Tiger,SGD,SGX,D05,100,40\n";
        let split = split("p.csv", csv.as_bytes(), day()).unwrap();
        let kinds: Vec<ImportKind> = split.parts.iter().map(|p| p.kind).collect();
        assert_eq!(kinds, vec![ImportKind::IndiaEquity, ImportKind::Global]);
    }

    #[test]
    fn a_fund_with_units_and_an_amount_gets_its_nav() {
        let csv = "Category,Account,Currency,Name,Quantity,Amount\nMUTUAL_FUND,Folio,INR,Some Fund,100,25000\n";
        let split = split("p.csv", csv.as_bytes(), day()).unwrap();
        assert!(split.parts[0].csv.contains(",250,"), "{}", split.parts[0].csv);
    }

    #[test]
    fn the_same_row_gets_the_same_reference() {
        assert_eq!(reference(&["a", "B"]), reference(&["A", "b"]));
        assert_ne!(reference(&["a"]), reference(&["b"]));
    }

    #[test]
    fn a_missing_required_column_is_refused() {
        assert!(split("p.csv", b"Account,Currency\nX,SGD\n", day()).is_err());
    }
}
