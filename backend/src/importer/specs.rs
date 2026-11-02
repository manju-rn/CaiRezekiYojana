//! The "correct format" for each upload, in one place.
//!
//! These specs are the single source of truth for three things: the template
//! the user downloads, the validation that runs on upload, and the format
//! documentation the UI renders. They cannot drift apart because there is
//! only one of them.

use serde::Serialize;

use crate::types::ImportKind;

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ColumnSpec {
    /// The canonical (normalised) column name.
    pub name: &'static str,
    /// The heading written into the downloadable template.
    pub heading: &'static str,
    /// Other headings accepted on upload, already normalised. Broker exports
    /// rarely match anyone's canonical naming.
    pub aliases: &'static [&'static str],
    pub required: bool,
    pub example: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct ImportSpec {
    pub kind: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub columns: &'static [ColumnSpec],
    /// Rows written into the template so the shape is obvious at a glance.
    pub sample_rows: &'static [&'static [&'static str]],
    pub notes: &'static [&'static str],
}

impl ImportSpec {
    /// Find the spec column an uploaded header maps to.
    pub fn column_for(&self, header: &str) -> Option<&ColumnSpec> {
        self.columns
            .iter()
            .find(|c| c.name == header || c.aliases.contains(&header))
    }

    /// Canonical name for an uploaded header, if we recognise it.
    pub fn canonical(&self, header: &str) -> Option<&'static str> {
        self.column_for(header).map(|c| c.name)
    }

    pub fn required_columns(&self) -> impl Iterator<Item = &ColumnSpec> {
        self.columns.iter().filter(|c| c.required)
    }

    /// The template, as CSV. Excel opens this directly; save-as .xlsx if you
    /// want a workbook. Generating CSV rather than xlsx keeps the binary free
    /// of a spreadsheet-writing dependency for no loss of usefulness.
    pub fn template_csv(&self) -> String {
        let mut out = String::new();
        let headings: Vec<String> = self.columns.iter().map(|c| csv_escape(c.heading)).collect();
        out.push_str(&headings.join(","));
        out.push('\n');
        for row in self.sample_rows {
            let cells: Vec<String> = row.iter().map(|c| csv_escape(c)).collect();
            out.push_str(&cells.join(","));
            out.push('\n');
        }
        out
    }
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

pub fn spec_for(kind: ImportKind) -> &'static ImportSpec {
    match kind {
        ImportKind::IndiaEquity => &INDIA_EQUITY,
        ImportKind::IndiaMf => &INDIA_MF,
        ImportKind::Global => &GLOBAL,
        ImportKind::Deposits => &DEPOSITS,
        ImportKind::CashBalances => &CASH_BALANCES,
        ImportKind::Instruments => &INSTRUMENTS,
    }
}

pub fn all_specs() -> Vec<&'static ImportSpec> {
    ImportKind::ALL.iter().map(|k| spec_for(*k)).collect()
}

// ---------------------------------------------------------------------------

const COMMON_NOTES: &[&str] = &[
    "The first row must be the column headings. Heading matching ignores case, spaces and punctuation.",
    "Dates may be YYYY-MM-DD, DD/MM/YYYY, DD-Mon-YYYY, or a native Excel date cell. Slash dates are read day-first.",
    "Amounts may contain currency symbols and thousands separators; (1,000) means -1000.",
    "Accounts and instruments named in the file are created automatically if they do not exist yet.",
    "Re-uploading a file is safe: a row already on file is skipped, not duplicated - matched by its Reference when it has one, otherwise by its contents (account, instrument, date, type, units and price; for deposits, amount, rate and term). A bank balance that has not changed is skipped too.",
];

pub static INDIA_EQUITY: ImportSpec = ImportSpec {
    kind: "INDIA_EQUITY",
    title: "Indian Stocks - transactions",
    description: "Buys, sells, dividends, bonuses and splits for NSE/BSE listed equity, ETFs and REITs.",
    columns: &[
        ColumnSpec { name: "date", heading: "Date", aliases: &["trade_date", "transaction_date", "txn_date", "order_date"], required: true, example: "2026-01-15", description: "Trade date." },
        ColumnSpec { name: "account", heading: "Account", aliases: &["demat_account", "broker", "account_name", "portfolio"], required: true, example: "Zerodha Demat", description: "Demat/trading account name. Created on first sight." },
        ColumnSpec { name: "symbol", heading: "Symbol", aliases: &["ticker", "scrip", "scrip_code", "stock", "security"], required: true, example: "INFY", description: "NSE/BSE ticker, without an exchange suffix." },
        ColumnSpec { name: "exchange", heading: "Exchange", aliases: &["exch"], required: false, example: "NSE", description: "NSE or BSE. Defaults to NSE." },
        ColumnSpec { name: "name", heading: "Name", aliases: &["company", "security_name", "scrip_name"], required: false, example: "Infosys Ltd", description: "Company name, used when the instrument is created." },
        ColumnSpec { name: "isin", heading: "ISIN", aliases: &[], required: false, example: "INE009A01021", description: "Helps match an existing instrument." },
        ColumnSpec { name: "type", heading: "Type", aliases: &["transaction_type", "txn_type", "action", "side", "buy_sell"], required: true, example: "BUY", description: "BUY, SELL, DIVIDEND, BONUS, SPLIT, TRANSFER_IN, TRANSFER_OUT." },
        ColumnSpec { name: "quantity", heading: "Quantity", aliases: &["qty", "shares", "units", "no_of_shares"], required: true, example: "50", description: "Shares traded. For SPLIT, the ratio multiplier (2 means 1-for-2)." },
        ColumnSpec { name: "price", heading: "Price", aliases: &["rate", "price_per_share", "avg_price", "unit_price"], required: true, example: "1450.25", description: "Price per share in INR. Use 0 for BONUS and SPLIT." },
        ColumnSpec { name: "fees", heading: "Fees", aliases: &["brokerage", "charges", "commission"], required: false, example: "23.60", description: "Brokerage and exchange charges. Added to cost basis." },
        ColumnSpec { name: "taxes", heading: "Taxes", aliases: &["stt", "tax", "gst", "stamp_duty"], required: false, example: "72.51", description: "STT, stamp duty, GST. Added to cost basis." },
        ColumnSpec { name: "reference", heading: "Reference", aliases: &["order_id", "trade_id", "contract_note", "ref"], required: false, example: "ZER-2026-000431", description: "Broker reference. Used to skip duplicates on re-upload." },
        ColumnSpec { name: "notes", heading: "Notes", aliases: &["remarks", "comment"], required: false, example: "", description: "Free text." },
    ],
    sample_rows: &[
        &["2026-01-15", "Zerodha Demat", "INFY", "NSE", "Infosys Ltd", "INE009A01021", "BUY", "50", "1450.25", "23.60", "72.51", "ZER-2026-000431", ""],
        &["2026-03-02", "Zerodha Demat", "TCS", "NSE", "Tata Consultancy Services", "INE467B01029", "BUY", "20", "3890.00", "18.40", "77.80", "ZER-2026-000512", ""],
        &["2026-06-20", "Zerodha Demat", "INFY", "NSE", "Infosys Ltd", "", "DIVIDEND", "50", "20.00", "0", "100.00", "DIV-INFY-2026", "Final dividend"],
        &["2026-07-11", "Zerodha Demat", "INFY", "NSE", "Infosys Ltd", "", "SELL", "20", "1610.00", "19.20", "32.20", "ZER-2026-000899", ""],
    ],
    notes: COMMON_NOTES,
};

pub static INDIA_MF: ImportSpec = ImportSpec {
    kind: "INDIA_MF",
    title: "Indian Mutual Funds - transactions",
    description: "Purchases, SIP instalments, redemptions, switches and IDCW for AMFI-registered schemes.",
    columns: &[
        ColumnSpec { name: "date", heading: "Date", aliases: &["trade_date", "transaction_date", "txn_date", "nav_date"], required: true, example: "2026-01-05", description: "Transaction date." },
        ColumnSpec { name: "account", heading: "Folio", aliases: &["folio", "folio_no", "folio_number", "account_name", "platform"], required: true, example: "HDFC Folio 12345678", description: "Folio or platform account. Created on first sight." },
        ColumnSpec { name: "scheme_code", heading: "AMFI Scheme Code", aliases: &["amfi_scheme_code", "amfi_code", "code"], required: false, example: "119551", description: "AMFI code. Strongly recommended - it is how NAV is matched." },
        ColumnSpec { name: "isin", heading: "ISIN", aliases: &[], required: false, example: "INF209K01157", description: "Alternative NAV match when no AMFI code is given." },
        ColumnSpec { name: "scheme_name", heading: "Scheme Name", aliases: &["scheme", "fund", "fund_name", "name", "security"], required: false, example: "Aditya Birla Sun Life Flexi Cap Fund - Growth", description: "Required when neither code nor ISIN is supplied." },
        ColumnSpec { name: "type", heading: "Type", aliases: &["transaction_type", "txn_type", "action"], required: true, example: "BUY", description: "BUY (or PURCHASE/SIP), SELL (or REDEMPTION), REINVEST, DIVIDEND, SWITCH_IN, SWITCH_OUT." },
        ColumnSpec { name: "units", heading: "Units", aliases: &["quantity", "qty", "no_of_units"], required: false, example: "32.456", description: "Units transacted. Derived from Amount / NAV when blank." },
        ColumnSpec { name: "nav", heading: "NAV", aliases: &["price", "purchase_nav", "rate", "nav_per_unit"], required: true, example: "1543.21", description: "NAV per unit on the transaction date." },
        ColumnSpec { name: "amount", heading: "Amount", aliases: &["gross_amount", "net_amount", "value"], required: false, example: "50000.00", description: "Gross amount. Used to derive units when Units is blank." },
        ColumnSpec { name: "fees", heading: "Fees", aliases: &["charges", "transaction_charge"], required: false, example: "0", description: "Transaction charges." },
        ColumnSpec { name: "taxes", heading: "Taxes", aliases: &["stamp_duty", "stt", "tax"], required: false, example: "2.50", description: "Stamp duty / STT." },
        ColumnSpec { name: "reference", heading: "Reference", aliases: &["ref", "transaction_id", "order_no"], required: false, example: "SIP-2026-01", description: "Registrar reference. Used to skip duplicates." },
        ColumnSpec { name: "notes", heading: "Notes", aliases: &["remarks"], required: false, example: "", description: "Free text." },
    ],
    sample_rows: &[
        &["2026-01-05", "HDFC Folio 12345678", "119551", "INF209K01157", "Aditya Birla Sun Life Flexi Cap Fund - Growth", "BUY", "32.456", "1543.21", "50000.00", "0", "2.50", "SIP-2026-01", "Monthly SIP"],
        &["2026-02-05", "HDFC Folio 12345678", "119551", "", "Aditya Birla Sun Life Flexi Cap Fund - Growth", "BUY", "", "1580.44", "50000.00", "0", "2.50", "SIP-2026-02", "Units derived from amount"],
        &["2026-03-18", "HDFC Folio 12345678", "118989", "INF179K01YV8", "HDFC Balanced Advantage Fund - Growth", "BUY", "195.300", "512.03", "100000.00", "0", "5.00", "LUMP-2026-03", ""],
        &["2026-08-22", "HDFC Folio 12345678", "119551", "", "", "SELL", "10.000", "1702.88", "17028.80", "0", "0", "RED-2026-08", "Partial redemption"],
    ],
    notes: COMMON_NOTES,
};

pub static GLOBAL: ImportSpec = ImportSpec {
    kind: "GLOBAL",
    title: "Global Stocks & ETFs - transactions",
    description: "Non-Indian equity and ETF trades. Each row carries its own trading currency, and optionally the FX rate that applied on the trade date.",
    columns: &[
        ColumnSpec { name: "date", heading: "Date", aliases: &["trade_date", "transaction_date", "settlement_date"], required: true, example: "2026-02-10", description: "Trade date." },
        ColumnSpec { name: "account", heading: "Account", aliases: &["broker", "account_name", "portfolio"], required: true, example: "IBKR Global", description: "Brokerage account name. Created on first sight." },
        ColumnSpec { name: "symbol", heading: "Symbol", aliases: &["ticker", "security", "stock"], required: true, example: "VOO", description: "Ticker as listed on the exchange." },
        ColumnSpec { name: "exchange", heading: "Exchange", aliases: &["exch", "market", "listing_exchange"], required: true, example: "NYSEARCA", description: "NASDAQ, NYSE, NYSEARCA, ASX, SGX, LSE, HKEX, TSX, XETRA, EURONEXT." },
        ColumnSpec { name: "name", heading: "Name", aliases: &["company", "security_name", "description"], required: false, example: "Vanguard S&P 500 ETF", description: "Used when the instrument is created." },
        ColumnSpec { name: "asset_kind", heading: "Asset Kind", aliases: &["kind", "asset_class", "type_of_asset", "instrument_type"], required: false, example: "ETF", description: "EQUITY, ETF, REIT or BOND. Defaults to EQUITY." },
        ColumnSpec { name: "isin", heading: "ISIN", aliases: &[], required: false, example: "US9229083632", description: "Optional identifier." },
        ColumnSpec { name: "type", heading: "Type", aliases: &["transaction_type", "txn_type", "action", "side", "buy_sell"], required: true, example: "BUY", description: "BUY, SELL, DIVIDEND, BONUS, SPLIT, TRANSFER_IN, TRANSFER_OUT." },
        ColumnSpec { name: "quantity", heading: "Quantity", aliases: &["qty", "shares", "units"], required: true, example: "12", description: "Shares or units. Fractional shares are fine." },
        ColumnSpec { name: "price", heading: "Price", aliases: &["rate", "unit_price", "price_per_share"], required: true, example: "512.40", description: "Price per share, in the row's Currency." },
        ColumnSpec { name: "currency", heading: "Currency", aliases: &["ccy", "trade_currency", "curr"], required: true, example: "USD", description: "ISO code the trade settled in: USD, SGD, AUD, GBP, HKD, EUR..." },
        ColumnSpec { name: "fees", heading: "Fees", aliases: &["commission", "brokerage", "charges"], required: false, example: "1.00", description: "In the row's Currency. Added to cost basis." },
        ColumnSpec { name: "taxes", heading: "Taxes", aliases: &["tax", "withholding_tax", "stamp_duty"], required: false, example: "0", description: "In the row's Currency." },
        ColumnSpec { name: "fx_rate", heading: "FX Rate", aliases: &["exchange_rate", "rate_to_base", "conversion_rate"], required: false, example: "87.90", description: "Units of base currency per 1 trade currency, on the trade date. Left blank, the current rate is used for reporting." },
        ColumnSpec { name: "reference", heading: "Reference", aliases: &["order_id", "trade_id", "ref", "confirmation"], required: false, example: "IBKR-88213", description: "Broker reference. Used to skip duplicates." },
        ColumnSpec { name: "notes", heading: "Notes", aliases: &["remarks", "comment"], required: false, example: "", description: "Free text." },
    ],
    sample_rows: &[
        &["2026-02-10", "IBKR Global", "VOO", "NYSEARCA", "Vanguard S&P 500 ETF", "ETF", "US9229083632", "BUY", "12", "512.40", "USD", "1.00", "0", "87.90", "IBKR-88213", ""],
        &["2026-03-04", "IBKR Global", "MSFT", "NASDAQ", "Microsoft Corp", "EQUITY", "", "BUY", "8", "425.10", "USD", "1.00", "0", "88.05", "IBKR-88790", ""],
        &["2026-04-18", "CommSec", "VAS", "ASX", "Vanguard Australian Shares ETF", "ETF", "", "BUY", "150", "104.25", "AUD", "9.95", "0", "58.20", "CS-2026-1180", ""],
        &["2026-05-09", "Tiger SG", "D05", "SGX", "DBS Group Holdings", "EQUITY", "", "BUY", "300", "38.90", "SGD", "2.50", "0", "65.40", "TG-55012", ""],
        &["2026-07-01", "IBKR Global", "VOO", "NYSEARCA", "", "", "", "DIVIDEND", "12", "1.78", "USD", "0", "3.20", "88.40", "DIV-VOO-Q2", "Quarterly distribution"],
    ],
    notes: COMMON_NOTES,
};

pub static DEPOSITS: ImportSpec = ImportSpec {
    kind: "DEPOSITS",
    title: "Fixed & recurring deposits",
    description: "Term deposits in any currency - Indian FDs and RDs, Singapore fixed deposits, Australian term deposits.",
    columns: &[
        ColumnSpec { name: "account", heading: "Account", aliases: &["account_name", "deposit_account", "portfolio"], required: true, example: "HDFC Deposits", description: "Grouping account for this institution's deposits. Created on first sight." },
        ColumnSpec { name: "institution", heading: "Institution", aliases: &["bank", "bank_name", "provider"], required: true, example: "HDFC Bank", description: "Bank or issuer." },
        ColumnSpec { name: "kind", heading: "Kind", aliases: &["deposit_type", "type", "product"], required: false, example: "FIXED_DEPOSIT", description: "FIXED_DEPOSIT, RECURRING_DEPOSIT or SCHEME. Defaults to FIXED_DEPOSIT." },
        ColumnSpec { name: "reference", heading: "Reference", aliases: &["deposit_no", "receipt_no", "fd_number", "ref"], required: false, example: "FD-8842-XXXX", description: "Masked deposit number. Used to skip duplicates." },
        ColumnSpec { name: "principal", heading: "Principal", aliases: &["amount", "deposit_amount", "invested"], required: true, example: "500000", description: "Lump sum deposited. Use 0 for a recurring deposit." },
        ColumnSpec { name: "installment", heading: "Instalment", aliases: &["instalment", "monthly_amount", "monthly_installment", "sip_amount"], required: false, example: "", description: "Monthly contribution, recurring deposits only." },
        ColumnSpec { name: "interest_rate", heading: "Interest Rate %", aliases: &["rate", "interest", "roi", "rate_pa", "interest_rate_pa"], required: true, example: "7.25", description: "Annual nominal rate as a percentage." },
        ColumnSpec { name: "compounding", heading: "Compounding", aliases: &["compound", "compounding_frequency"], required: false, example: "QUARTERLY", description: "SIMPLE, MONTHLY, QUARTERLY, HALF_YEARLY, ANNUAL, AT_MATURITY. Defaults to QUARTERLY." },
        ColumnSpec { name: "payout", heading: "Payout", aliases: &["payout_frequency", "interest_payout", "payout_type"], required: false, example: "CUMULATIVE", description: "CUMULATIVE (reinvested) or MONTHLY/QUARTERLY/HALF_YEARLY/ANNUAL (paid out)." },
        ColumnSpec { name: "start_date", heading: "Start Date", aliases: &["opened_on", "deposit_date", "from_date", "date"], required: true, example: "2026-04-01", description: "Date the deposit started." },
        ColumnSpec { name: "maturity_date", heading: "Maturity Date", aliases: &["maturity", "to_date", "end_date", "matures_on"], required: true, example: "2029-04-01", description: "Date the deposit matures." },
        ColumnSpec { name: "currency", heading: "Currency", aliases: &["ccy", "curr"], required: true, example: "INR", description: "INR, SGD, AUD or any other ISO code." },
        ColumnSpec { name: "country", heading: "Country", aliases: &["country_code"], required: false, example: "IN", description: "IN, SG, AU. Inferred from the currency when blank." },
        ColumnSpec { name: "maturity_amount", heading: "Maturity Amount", aliases: &["maturity_value", "expected_maturity"], required: false, example: "617000", description: "The bank's quoted maturity value. Overrides our calculation when given." },
        ColumnSpec { name: "tds_pct", heading: "TDS %", aliases: &["tds", "tds_rate", "tax_withheld", "withholding_tax"], required: false, example: "", description: "Tax withheld at source on the interest, as a percentage - 31.2 for an Indian NRO deposit. Taken out of the deposit each 31 March. Blank when none is withheld." },
        ColumnSpec { name: "auto_renew", heading: "Auto Renew", aliases: &["renew", "auto_renewal"], required: false, example: "N", description: "Y or N." },
        ColumnSpec { name: "notes", heading: "Notes", aliases: &["remarks"], required: false, example: "", description: "Free text." },
    ],
    sample_rows: &[
        &["HDFC Deposits", "HDFC Bank", "FIXED_DEPOSIT", "FD-8842-XXXX", "500000", "", "7.25", "QUARTERLY", "CUMULATIVE", "2026-04-01", "2029-04-01", "INR", "IN", "", "", "N", "3-year tax-saver"],
        &["SBI Deposits", "State Bank of India", "RECURRING_DEPOSIT", "RD-1190-XXXX", "0", "10000", "6.80", "QUARTERLY", "CUMULATIVE", "2026-01-10", "2028-01-10", "INR", "IN", "", "", "N", "Monthly RD"],
        &["DBS Singapore", "DBS Bank", "FIXED_DEPOSIT", "DBS-FD-2211", "50000", "", "3.10", "AT_MATURITY", "CUMULATIVE", "2026-05-15", "2027-05-15", "SGD", "SG", "51550", "", "Y", "12-month promo rate"],
        &["ANZ Term Deposits", "ANZ", "FIXED_DEPOSIT", "ANZ-TD-7781", "75000", "", "4.45", "AT_MATURITY", "QUARTERLY", "2026-06-01", "2027-06-01", "AUD", "AU", "", "", "N", "Interest paid quarterly"],
    ],
    notes: COMMON_NOTES,
};

pub static CASH_BALANCES: ImportSpec = ImportSpec {
    kind: "CASH_BALANCES",
    title: "Cash balances",
    description: "Point-in-time balances for transaction and savings accounts. Upload again whenever you want to record a new snapshot.",
    columns: &[
        ColumnSpec { name: "account", heading: "Account", aliases: &["account_name", "bank_account"], required: true, example: "HDFC Savings", description: "Account name. Created on first sight." },
        ColumnSpec { name: "institution", heading: "Institution", aliases: &["bank", "bank_name"], required: false, example: "HDFC Bank", description: "Bank name." },
        ColumnSpec { name: "as_of", heading: "As Of", aliases: &["date", "balance_date", "statement_date"], required: true, example: "2026-09-18", description: "Date this balance was true." },
        ColumnSpec { name: "balance", heading: "Balance", aliases: &["amount", "closing_balance", "available_balance"], required: true, example: "185400.50", description: "Closing balance in the account's currency." },
        ColumnSpec { name: "currency", heading: "Currency", aliases: &["ccy", "curr"], required: true, example: "INR", description: "INR, SGD, AUD or any other ISO code." },
        ColumnSpec { name: "country", heading: "Country", aliases: &["country_code"], required: false, example: "IN", description: "IN, SG, AU. Inferred from the currency when blank." },
        ColumnSpec { name: "notes", heading: "Notes", aliases: &["remarks"], required: false, example: "", description: "Free text." },
    ],
    sample_rows: &[
        &["HDFC Savings", "HDFC Bank", "2026-09-18", "185400.50", "INR", "IN", ""],
        &["DBS Multiplier", "DBS Bank", "2026-09-18", "24850.00", "SGD", "SG", ""],
        &["ANZ Everyday", "ANZ", "2026-09-18", "9120.75", "AUD", "AU", ""],
    ],
    notes: COMMON_NOTES,
};

pub static INSTRUMENTS: ImportSpec = ImportSpec {
    kind: "INSTRUMENTS",
    title: "Instrument master (optional)",
    description: "Pre-load instrument details so transaction imports pick up proper names, ISINs and AMFI codes automatically.",
    columns: &[
        ColumnSpec { name: "symbol", heading: "Symbol", aliases: &["ticker", "code"], required: true, example: "INFY", description: "Ticker, or AMFI scheme code for a fund." },
        ColumnSpec { name: "name", heading: "Name", aliases: &["security_name", "company", "scheme_name"], required: true, example: "Infosys Ltd", description: "Display name." },
        ColumnSpec { name: "kind", heading: "Kind", aliases: &["asset_kind", "asset_class", "type"], required: true, example: "EQUITY", description: "EQUITY, ETF, MUTUAL_FUND, BOND, REIT, COMMODITY." },
        ColumnSpec { name: "exchange", heading: "Exchange", aliases: &["exch", "market"], required: false, example: "NSE", description: "Exchange code. Use AMFI for Indian mutual funds." },
        ColumnSpec { name: "currency", heading: "Currency", aliases: &["ccy"], required: false, example: "INR", description: "Pricing currency. Inferred from the exchange when blank." },
        ColumnSpec { name: "country", heading: "Country", aliases: &["country_code"], required: false, example: "IN", description: "Inferred from the exchange when blank." },
        ColumnSpec { name: "isin", heading: "ISIN", aliases: &[], required: false, example: "INE009A01021", description: "Optional identifier." },
        ColumnSpec { name: "amfi_scheme_code", heading: "AMFI Scheme Code", aliases: &["amfi_code", "scheme_code"], required: false, example: "", description: "Mutual funds only - the NAV match key." },
        ColumnSpec { name: "quote_symbol", heading: "Quote Symbol", aliases: &["yahoo_symbol", "provider_symbol"], required: false, example: "INFY.NS", description: "Override the symbol sent to the quote provider." },
        ColumnSpec { name: "sector", heading: "Sector", aliases: &["industry"], required: false, example: "Information Technology", description: "Free text, used for grouping." },
    ],
    sample_rows: &[
        &["INFY", "Infosys Ltd", "EQUITY", "NSE", "INR", "IN", "INE009A01021", "", "INFY.NS", "Information Technology"],
        &["119551", "Aditya Birla Sun Life Flexi Cap Fund - Growth", "MUTUAL_FUND", "AMFI", "INR", "IN", "INF209K01157", "119551", "", "Flexi Cap"],
        &["VOO", "Vanguard S&P 500 ETF", "ETF", "NYSEARCA", "USD", "US", "US9229083632", "", "VOO", "Broad Market"],
    ],
    notes: COMMON_NOTES,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_a_spec_and_matching_sample_widths() {
        for spec in all_specs() {
            assert!(!spec.columns.is_empty(), "{} has no columns", spec.kind);
            for (i, row) in spec.sample_rows.iter().enumerate() {
                assert_eq!(
                    row.len(),
                    spec.columns.len(),
                    "{} sample row {} has {} cells but {} columns",
                    spec.kind,
                    i,
                    row.len(),
                    spec.columns.len()
                );
            }
        }
    }

    #[test]
    fn headings_normalise_onto_their_own_canonical_names() {
        use crate::importer::reader::normalise_header;
        for spec in all_specs() {
            for col in spec.columns {
                let normalised = normalise_header(col.heading);
                assert!(
                    spec.canonical(&normalised) == Some(col.name),
                    "{}: heading '{}' normalises to '{}' which does not map back to '{}'",
                    spec.kind,
                    col.heading,
                    normalised,
                    col.name
                );
            }
        }
    }

    #[test]
    fn aliases_are_already_normalised_and_unambiguous() {
        use crate::importer::reader::normalise_header;
        for spec in all_specs() {
            let mut seen: Vec<&str> = Vec::new();
            for col in spec.columns {
                for alias in col.aliases {
                    assert_eq!(normalise_header(alias), *alias, "{}: alias '{}' is not normalised", spec.kind, alias);
                    assert!(!seen.contains(alias), "{}: alias '{}' is claimed twice", spec.kind, alias);
                    seen.push(alias);
                }
                assert!(!seen.contains(&col.name), "{}: '{}' is both a name and an alias", spec.kind, col.name);
                seen.push(col.name);
            }
        }
    }

    #[test]
    fn templates_round_trip_through_the_csv_reader() {
        use crate::importer::reader::read;
        for spec in all_specs() {
            let csv = spec.template_csv();
            let sheet = read("template.csv", csv.as_bytes()).unwrap();
            assert_eq!(sheet.rows.len(), spec.sample_rows.len(), "{}", spec.kind);
            for col in spec.required_columns() {
                assert!(
                    sheet.headers.iter().any(|h| spec.canonical(h) == Some(col.name)),
                    "{}: template is missing required column '{}'",
                    spec.kind,
                    col.name
                );
            }
        }
    }

    #[test]
    fn csv_escaping_handles_commas_in_fund_names() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
    }
}
