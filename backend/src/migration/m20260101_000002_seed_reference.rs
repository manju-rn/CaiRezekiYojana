use sea_orm_migration::prelude::*;

use super::idens::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

/// (code, name, symbol, minor_units)
const CURRENCIES: &[(&str, &str, &str, i32)] = &[
    ("INR", "Indian Rupee", "\u{20B9}", 2),
    ("SGD", "Singapore Dollar", "S$", 2),
    ("AUD", "Australian Dollar", "A$", 2),
    ("USD", "US Dollar", "$", 2),
    ("GBP", "Pound Sterling", "\u{A3}", 2),
    ("EUR", "Euro", "\u{20AC}", 2),
    ("HKD", "Hong Kong Dollar", "HK$", 2),
    ("JPY", "Japanese Yen", "\u{A5}", 0),
    ("CAD", "Canadian Dollar", "C$", 2),
    ("CHF", "Swiss Franc", "CHF", 2),
    ("AED", "UAE Dirham", "\u{62F}.\u{625}", 2),
    ("NZD", "New Zealand Dollar", "NZ$", 2),
];

/// (code, name, country, currency, mic, quote_suffix, timezone)
const EXCHANGES: &[(&str, &str, &str, &str, &str, &str, &str)] = &[
    ("NSE", "National Stock Exchange of India", "IN", "INR", "XNSE", ".NS", "Asia/Kolkata"),
    ("BSE", "BSE Limited", "IN", "INR", "XBOM", ".BO", "Asia/Kolkata"),
    // Pseudo-exchange so Indian mutual-fund schemes hang off something real.
    ("AMFI", "AMFI (Indian Mutual Funds)", "IN", "INR", "", "", "Asia/Kolkata"),
    ("NASDAQ", "Nasdaq Stock Market", "US", "USD", "XNAS", "", "America/New_York"),
    ("NYSE", "New York Stock Exchange", "US", "USD", "XNYS", "", "America/New_York"),
    ("NYSEARCA", "NYSE Arca", "US", "USD", "ARCX", "", "America/New_York"),
    ("BATS", "Cboe BZX", "US", "USD", "BATS", "", "America/New_York"),
    ("LSE", "London Stock Exchange", "GB", "GBP", "XLON", ".L", "Europe/London"),
    ("SGX", "Singapore Exchange", "SG", "SGD", "XSES", ".SI", "Asia/Singapore"),
    ("ASX", "Australian Securities Exchange", "AU", "AUD", "XASX", ".AX", "Australia/Sydney"),
    ("HKEX", "Hong Kong Exchanges", "HK", "HKD", "XHKG", ".HK", "Asia/Hong_Kong"),
    ("TSX", "Toronto Stock Exchange", "CA", "CAD", "XTSE", ".TO", "America/Toronto"),
    ("XETRA", "Deutsche Boerse Xetra", "DE", "EUR", "XETR", ".DE", "Europe/Berlin"),
    ("EURONEXT", "Euronext Amsterdam", "NL", "EUR", "XAMS", ".AS", "Europe/Amsterdam"),
];

/// Defaults the app boots with. All are editable from Settings.
const SETTINGS: &[(&str, &str)] = &[
    ("base_currency", "SGD"),
    ("cost_basis_method", "FIFO"),
    ("fx_provider", "frankfurter"),
    ("quote_provider", "yahoo"),
    ("nav_provider", "amfi"),
    ("xe_api_id", ""),
    ("xe_api_key", ""),
    ("auto_refresh_enabled", "false"),
    ("financial_year_start_month", "4"),
];

/// Pairs pre-wired for conversion into the default INR base, plus the
/// SGD/AUD cross that a Singapore+Australia portfolio actually needs.
const FX_PAIRS: &[(&str, &str)] = &[
    ("USD", "INR"),
    ("SGD", "INR"),
    ("AUD", "INR"),
    ("GBP", "INR"),
    ("EUR", "INR"),
    ("HKD", "INR"),
    ("CAD", "INR"),
    ("JPY", "INR"),
    ("AUD", "SGD"),
    ("SGD", "AUD"),
    ("USD", "SGD"),
    ("USD", "AUD"),
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, m: &SchemaManager) -> Result<(), DbErr> {
        let now = chrono::Utc::now().naive_utc();

        let mut cur = Query::insert();
        cur.into_table(Currency::Table).columns([
            Currency::Code,
            Currency::Name,
            Currency::Symbol,
            Currency::MinorUnits,
            Currency::IsActive,
        ]);
        for (code, name, symbol, minor) in CURRENCIES {
            cur.values_panic([
                (*code).into(),
                (*name).into(),
                (*symbol).into(),
                (*minor).into(),
                true.into(),
            ]);
        }
        m.exec_stmt(cur.to_owned()).await?;

        let mut ex = Query::insert();
        ex.into_table(Exchange::Table).columns([
            Exchange::Code,
            Exchange::Name,
            Exchange::Country,
            Exchange::CurrencyCode,
            Exchange::Mic,
            Exchange::QuoteSuffix,
            Exchange::Timezone,
        ]);
        for (code, name, country, ccy, mic, suffix, tz) in EXCHANGES {
            ex.values_panic([
                (*code).into(),
                (*name).into(),
                (*country).into(),
                (*ccy).into(),
                opt(mic).into(),
                opt(suffix).into(),
                (*tz).into(),
            ]);
        }
        m.exec_stmt(ex.to_owned()).await?;

        let mut st = Query::insert();
        st.into_table(AppSetting::Table).columns([
            AppSetting::Key,
            AppSetting::Value,
            AppSetting::UpdatedAt,
        ]);
        for (k, v) in SETTINGS {
            st.values_panic([(*k).into(), (*v).into(), now.into()]);
        }
        m.exec_stmt(st.to_owned()).await?;

        let mut fx = Query::insert();
        fx.into_table(FxSetting::Table).columns([
            FxSetting::BaseCode,
            FxSetting::QuoteCode,
            FxSetting::Provider,
            FxSetting::MarginMode,
            FxSetting::MarginValue,
            FxSetting::UseManual,
            FxSetting::UpdatedAt,
        ]);
        for (base, quote) in FX_PAIRS {
            fx.values_panic([
                (*base).into(),
                (*quote).into(),
                "default".into(),
                "NONE".into(),
                0.0f64.into(),
                false.into(),
                now.into(),
            ]);
        }
        m.exec_stmt(fx.to_owned()).await?;

        Ok(())
    }

    async fn down(&self, m: &SchemaManager) -> Result<(), DbErr> {
        for stmt in [
            Query::delete().from_table(FxSetting::Table).to_owned(),
            Query::delete().from_table(AppSetting::Table).to_owned(),
            Query::delete().from_table(Exchange::Table).to_owned(),
            Query::delete().from_table(Currency::Table).to_owned(),
        ] {
            m.exec_stmt(stmt).await?;
        }
        Ok(())
    }
}

/// Empty strings in the seed tables mean "no value", not "blank value".
fn opt(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}
