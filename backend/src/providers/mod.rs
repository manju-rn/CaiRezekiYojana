//! Outbound data feeds.
//!
//! Every feed sits behind a trait and is chosen at runtime from
//! `app_setting`, so swapping Yahoo for a paid quote vendor, or the free FX
//! feed for Xe, is a settings change rather than a code change.
//!
//! No provider is ever allowed to be fatal. A failed refresh leaves the last
//! stored value in place and surfaces a warning; the portfolio still renders.

pub mod fx_frankfurter;
pub mod fx_exchangerate_host;
pub mod fx_fees_wise;
pub mod fx_xe;
pub mod holdings_groww;
pub mod holdings_yahoo;
pub mod nav_amfi;
pub mod nav_history_mfapi;
pub mod quotes_yahoo;

use async_trait::async_trait;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::entities::instrument;
use crate::error::AppResult;
use crate::state::AppState;

/// A price for one instrument.
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Quote {
    /// The provider-facing symbol that was requested, e.g. `INFY.NS`.
    pub symbol: String,
    pub price: f64,
    pub currency: String,
    pub as_of: NaiveDate,
    /// What the provider says the symbol is - `EQUITY`, `ETF`, ... - when
    /// it says. Lets a refresh correct a fund imported as a share.
    #[serde(default)]
    pub instrument_type: Option<String>,
}

/// A NAV for one Indian mutual-fund scheme.
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Nav {
    pub scheme_code: String,
    pub scheme_name: String,
    pub isin: Option<String>,
    pub nav: f64,
    pub as_of: NaiveDate,
    /// The AMFI section heading the scheme sits under, e.g.
    /// `Equity Scheme - Large Cap Fund`. Drives the Equity/Debt split.
    pub category: Option<String>,
}

impl crate::domain::scheme::Candidate for Nav {
    fn scheme_name(&self) -> &str {
        &self.scheme_name
    }
}

/// One stored daily close. The shape the history backfills speak, whichever
/// feed they came from - so `price_point` is filled the same way for a fund
/// and for an equity.
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DailyClose {
    pub as_of: NaiveDate,
    pub close: f64,
}

/// `1 base = rate quote`.
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FxQuote {
    pub base: String,
    pub quote: String,
    pub rate: f64,
    pub as_of: NaiveDate,
}

#[async_trait]
pub trait QuoteProvider: Send + Sync {
    fn id(&self) -> &'static str;

    /// Fetch prices for `symbols`. Partial success is normal and expected:
    /// a delisted ticker should not sink the other forty.
    async fn quotes(&self, state: &AppState, symbols: &[String]) -> AppResult<Vec<Quote>>;
}

#[async_trait]
pub trait NavProvider: Send + Sync {
    fn id(&self) -> &'static str;

    /// The whole NAV universe for the day. AMFI publishes one file covering
    /// every scheme, so fetching all of it once beats per-scheme lookups.
    async fn all_navs(&self, state: &AppState) -> AppResult<Vec<Nav>>;
}

#[async_trait]
pub trait FxProvider: Send + Sync {
    fn id(&self) -> &'static str;

    /// Human-readable name for the Forex tab.
    fn label(&self) -> &'static str;

    /// True when the provider needs credentials that are not configured yet.
    async fn is_configured(&self, state: &AppState) -> bool {
        let _ = state;
        true
    }

    async fn rates(&self, state: &AppState, base: &str, quotes: &[String]) -> AppResult<Vec<FxQuote>>;
}

/// Resolve the FX provider named in settings, falling back to the free feed.
pub fn fx_provider(id: &str) -> Box<dyn FxProvider> {
    match id {
        "xe" => Box::new(fx_xe::XeProvider),
        "exchangerate_host" => Box::new(fx_exchangerate_host::ExchangeRateHostProvider),
        _ => Box::new(fx_frankfurter::FrankfurterProvider),
    }
}

pub fn quote_provider(_id: &str) -> Box<dyn QuoteProvider> {
    // Only one quote provider today; the indirection is here so adding a
    // paid vendor later is a match arm rather than a refactor.
    Box::new(quotes_yahoo::YahooProvider)
}

pub fn nav_provider(_id: &str) -> Box<dyn NavProvider> {
    Box::new(nav_amfi::AmfiProvider)
}

/// One line of a fund's disclosed portfolio.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[schema(as = DisclosedFundHolding)]
pub struct FundHolding {
    pub name: String,
    /// `EQUITY`, `DEBT`, `CASH`, `MF` or `OTHER`.
    pub nature: String,
    /// Finer type: `Equity`, `Certificate of Deposit`, `Repo`, ...
    pub instrument_type: Option<String>,
    pub sector: Option<String>,
    /// Share of the fund's assets, 0-100. Net payables can go negative.
    pub weight_pct: f64,
    /// What the fund holds of the security, in INR.
    pub market_value: Option<f64>,
    /// The provider's identifier for the security, when it has one.
    pub security_ref: Option<String>,
}

/// `instrument_type` of a line standing for the part of a portfolio the
/// feed does not name security by security - what a sector holds beyond
/// the fund's top holdings, say.
pub const REMAINDER_TYPE: &str = "Not itemised";

/// A fund's portfolio as disclosed for one month-end.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FundPortfolio {
    pub as_of: NaiveDate,
    /// The provider's identifier for the fund. Remembered on the instrument
    /// so the next refresh skips the search.
    pub fund_ref: String,
    pub holdings: Vec<FundHolding>,
}

#[async_trait]
pub trait HoldingsProvider: Send + Sync {
    fn id(&self) -> &'static str;

    /// The fund's latest disclosed portfolio, or `None` when the provider
    /// has nothing it can confidently say is this fund. A fund it knows
    /// but has no portfolio for is an error, so the refresh can report it.
    async fn portfolio(
        &self,
        state: &AppState,
        fund: &instrument::Model,
    ) -> AppResult<Option<FundPortfolio>>;
}

pub fn holdings_provider(_id: &str) -> Box<dyn HoldingsProvider> {
    // One source today. Another disclosure feed is a match arm here.
    Box::new(holdings_groww::GrowwProvider)
}

/// Portfolios of the ETFs and funds listed outside India.
pub fn etf_holdings_provider() -> Box<dyn HoldingsProvider> {
    Box::new(holdings_yahoo::YahooHoldingsProvider)
}

/// Everything the Forex tab offers in its provider dropdown.
#[derive(Serialize, utoipa::ToSchema)]
#[schema(as = FxProviderInfo)]
pub struct ProviderInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub configured: bool,
    pub notes: &'static str,
}

pub async fn fx_provider_catalog(state: &AppState) -> Vec<ProviderInfo> {
    let mut out = Vec::new();
    for id in ["frankfurter", "exchangerate_host", "xe"] {
        let p = fx_provider(id);
        out.push(ProviderInfo {
            id: p.id(),
            label: p.label(),
            configured: p.is_configured(state).await,
            notes: match id {
                "xe" => "Commercial XE Currency Data API. Set xe_api_id and xe_api_key in Settings.",
                "exchangerate_host" => "Free tier; may require an access key depending on plan.",
                _ => "Free ECB-sourced reference rates. No key required. Default.",
            },
        });
    }
    out
}
