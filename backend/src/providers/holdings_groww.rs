//! Mutual-fund portfolio disclosures, via Groww.
//!
//! Every AMC publishes its schemes' portfolios monthly, each in its own
//! spreadsheet layout, and AMFI does not collect them. Groww's public web
//! API serves those disclosures already normalised - one row per security
//! with the share of the fund it makes up - which is exactly what the
//! look-through view needs. It is an unofficial endpoint, so it sits behind
//! the `HoldingsProvider` trait and a failure only ever costs a refresh.
//!
//! Finding the fund is the delicate part. The search endpoint returns the
//! AMFI scheme code with each hit and the scheme page carries the ISINs, so
//! a hit is used only when one of those agrees with what the instrument
//! already knows. A name that looks right is not enough - "Large Cap" alone
//! matches forty schemes.

use std::collections::HashSet;

use async_trait::async_trait;
use chrono::NaiveDate;
use serde_json::Value;

use crate::domain::scheme;
use crate::entities::instrument;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

use super::{nav_history_mfapi, FundHolding, FundPortfolio, HoldingsProvider};

const PROVIDER: &str = "groww";
const SEARCH_URL: &str = "https://groww.in/v1/api/search/v1/entity";
const SCHEME_URL: &str = "https://groww.in/v1/api/data/mf/web/v1/scheme/search";

/// Hits asked for per search. The right scheme is almost always first;
/// past a handful the rest are other houses' funds in the same category.
const SEARCH_SIZE: usize = 8;

/// Scheme pages opened to confirm a hit by ISIN when no hit carries the
/// fund's code. Each is a request, so only the top few are tried.
const MAX_ISIN_PROBES: usize = 3;

/// Groww reports each holding's value in rupees crore.
const CRORE: f64 = 10_000_000.0;

pub struct GrowwProvider;

#[async_trait]
impl HoldingsProvider for GrowwProvider {
    fn id(&self) -> &'static str {
        PROVIDER
    }

    async fn portfolio(
        &self,
        state: &AppState,
        fund: &instrument::Model,
    ) -> AppResult<Option<FundPortfolio>> {
        // A reference already on the instrument - learned from an earlier
        // confirmed match, or set by hand - goes straight to the page.
        if let Some(fund_ref) = fund.holdings_ref.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
            let page = fetch_page(state, fund_ref).await?;
            return Ok(Some(to_portfolio(&page, fund_ref)?));
        }

        // Search by the statement's name and, if that finds nothing with
        // the fund's code, by the name AMFI publishes for the code.
        let mut probes: Vec<SearchHit> = Vec::new();
        let mut queries = vec![scheme::search_phrase(&fund.name)];
        let mut i = 0;
        while i < queries.len() {
            let hits = search(state, &queries[i]).await?;
            if let Some(hit) = hits.iter().find(|h| h.matches_code(fund)) {
                let page = fetch_page(state, &hit.search_id).await?;
                if is_same_fund(&page, fund) {
                    return Ok(Some(to_portfolio(&page, &hit.search_id)?));
                }
            }
            probes.extend(hits);
            i += 1;

            if i == 1 {
                if let Some(code) = fund.amfi_scheme_code.as_deref() {
                    // mfapi being down should not cost the ISIN attempt below.
                    if let Some(official) = nav_history_mfapi::scheme_name(state, code).await.ok().flatten() {
                        let q = scheme::search_phrase(&official);
                        if !queries.contains(&q) {
                            queries.push(q);
                        }
                    }
                }
            }
        }

        // No hit carried the fund's code - a fund with no code, or one the
        // feed lists under another plan's code. Open the top hits and check
        // their ISINs instead.
        let mut seen = HashSet::new();
        for hit in probes.into_iter().filter(|h| seen.insert(h.search_id.clone())).take(MAX_ISIN_PROBES) {
            let page = fetch_page(state, &hit.search_id).await?;
            if is_same_fund(&page, fund) {
                return Ok(Some(to_portfolio(&page, &hit.search_id)?));
            }
        }
        Ok(None)
    }
}

/// One result from the search endpoint.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchHit {
    /// The slug the scheme page is fetched by.
    pub search_id: String,
    /// The AMFI scheme code, for most schemes.
    pub scheme_code: Option<String>,
    pub title: String,
}

impl SearchHit {
    fn matches_code(&self, fund: &instrument::Model) -> bool {
        match (&self.scheme_code, &fund.amfi_scheme_code) {
            (Some(hit), Some(mine)) => hit == mine,
            _ => false,
        }
    }
}

/// The parts of a scheme page this provider reads.
#[derive(Clone, Debug, Default)]
pub struct SchemePage {
    pub scheme_name: Option<String>,
    pub scheme_code: Option<String>,
    pub direct_scheme_code: Option<String>,
    pub regular_scheme_code: Option<String>,
    pub isin: Option<String>,
    pub regular_isin: Option<String>,
    pub holdings: Vec<ParsedHolding>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedHolding {
    pub as_of: Option<NaiveDate>,
    pub holding: FundHolding,
}

async fn search(state: &AppState, query: &str) -> AppResult<Vec<SearchHit>> {
    let size = SEARCH_SIZE.to_string();
    let res = state
        .http
        .get(SEARCH_URL)
        .query(&[
            ("app", "false"),
            ("entity_type", "scheme"),
            ("page", "0"),
            ("q", query),
            ("size", size.as_str()),
        ])
        .send()
        .await?;
    if !res.status().is_success() {
        return Err(AppError::provider(PROVIDER, format!("search: HTTP {}", res.status())));
    }
    let body = res.text().await?;
    Ok(parse_search(&body))
}

async fn fetch_page(state: &AppState, fund_ref: &str) -> AppResult<SchemePage> {
    if !is_slug(fund_ref) {
        return Err(AppError::provider(
            PROVIDER,
            format!(
                "'{fund_ref}' is not a Groww scheme reference - use the last part of the scheme's \
                 groww.in URL, e.g. hdfc-mid-cap-fund-direct-growth"
            ),
        ));
    }
    let res = state.http.get(format!("{SCHEME_URL}/{fund_ref}")).send().await?;
    if !res.status().is_success() {
        return Err(AppError::provider(PROVIDER, format!("scheme page '{fund_ref}': HTTP {}", res.status())));
    }
    let body = res.text().await?;
    parse_page(&body)
}

fn is_slug(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The search response: `{"content": [{"search_id", "scheme_code", "title", ...}]}`.
pub fn parse_search(body: &str) -> Vec<SearchHit> {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    v["content"]
        .as_array()
        .map(|hits| {
            hits.iter()
                .filter_map(|h| {
                    Some(SearchHit {
                        search_id: text(&h["search_id"])?,
                        scheme_code: text(&h["scheme_code"]),
                        title: text(&h["title"]).unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The scheme page. Identifiers are read by name; the holdings are a list
/// of positional arrays, one per security - see [`parse_holding`]. A page
/// for an unknown slug still returns 200, with every field null, so an
/// unreadable body is the only hard failure here.
pub fn parse_page(body: &str) -> AppResult<SchemePage> {
    let v: Value = serde_json::from_str(body)
        .map_err(|e| AppError::provider(PROVIDER, format!("unreadable scheme page: {e}")))?;
    let holdings = v["holdings"]
        .as_array()
        .map(|rows| rows.iter().filter_map(parse_holding).collect())
        .unwrap_or_default();
    Ok(SchemePage {
        scheme_name: text(&v["scheme_name"]),
        scheme_code: text(&v["scheme_code"]),
        direct_scheme_code: text(&v["direct_scheme_code"]),
        regular_scheme_code: text(&v["regular_scheme_code"]),
        isin: text(&v["isin"]),
        regular_isin: text(&v["regular_isin"]),
        holdings,
    })
}

/// A holding row is a positional array:
///
/// ```text
/// [scheme code, disclosure date, security name, nature, sector,
///  instrument type, -, market value in crore, share of assets %, -, -, security id]
/// ```
///
/// Only what is needed is read, by position; a row with no name or no
/// share is dropped rather than failing the page. A sector of
/// `Unspecified` is the feed's way of saying none.
fn parse_holding(row: &Value) -> Option<ParsedHolding> {
    let cells = row.as_array()?;
    let cell = |i: usize| cells.get(i).unwrap_or(&Value::Null);
    let name = text(cell(2))?;
    let weight_pct = number(cell(8))?;
    Some(ParsedHolding {
        as_of: text(cell(1)).and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()),
        holding: FundHolding {
            name,
            nature: text(cell(3)).map(|n| n.to_ascii_uppercase()).unwrap_or_else(|| "OTHER".to_string()),
            instrument_type: text(cell(5)),
            sector: text(cell(4)).filter(|s| !s.eq_ignore_ascii_case("unspecified")),
            weight_pct,
            market_value: number(cell(7)).map(|crore| crore * CRORE),
            security_ref: text(cell(11)),
        },
    })
}

fn text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => {
            let t = s.trim();
            (!t.is_empty()).then(|| t.to_string())
        }
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Whether a scheme page is the fund: its AMFI code, or one of its ISINs,
/// must agree with what the instrument carries. A fund imported from a
/// statement often has its ISIN as its symbol and nothing else.
pub fn is_same_fund(page: &SchemePage, fund: &instrument::Model) -> bool {
    let code_ok = fund.amfi_scheme_code.as_deref().is_some_and(|code| {
        [&page.scheme_code, &page.direct_scheme_code, &page.regular_scheme_code]
            .iter()
            .any(|c| c.as_deref() == Some(code))
    });
    let mine: Vec<&str> = fund
        .isin
        .iter()
        .chain(std::iter::once(&fund.symbol))
        .map(String::as_str)
        .filter(|s| looks_like_isin(s))
        .collect();
    let isin_ok = [&page.isin, &page.regular_isin]
        .iter()
        .any(|p| p.as_deref().is_some_and(|theirs| mine.iter().any(|m| m.eq_ignore_ascii_case(theirs))));
    code_ok || isin_ok
}

fn looks_like_isin(s: &str) -> bool {
    s.len() == 12 && s.chars().all(|c| c.is_ascii_alphanumeric())
}

/// The page's holdings as a portfolio, dated by the disclosure they carry.
/// A page with none is an error rather than an empty portfolio: the fund
/// was found, so silence would read as "holds nothing".
fn to_portfolio(page: &SchemePage, fund_ref: &str) -> AppResult<FundPortfolio> {
    if page.holdings.is_empty() {
        return Err(AppError::provider(PROVIDER, format!("'{fund_ref}' has no portfolio on its page")));
    }
    let as_of = page
        .holdings
        .iter()
        .filter_map(|h| h.as_of)
        .max()
        .ok_or_else(|| AppError::provider(PROVIDER, format!("'{fund_ref}': portfolio rows carry no disclosure date")))?;
    Ok(FundPortfolio {
        as_of,
        fund_ref: fund_ref.to_string(),
        holdings: page.holdings.iter().map(|h| h.holding.clone()).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH: &str = r#"{"content":[
      {"entity_type":"Scheme","id":"hdfc-mid-cap-fund-direct-growth","scheme_code":"118989","search_id":"hdfc-mid-cap-fund-direct-growth","title":"HDFC Mid Cap Fund"},
      {"entity_type":"Scheme","id":"hdfc-large-mid-cap-fund-direct-growth","scheme_code":"130498","search_id":"hdfc-large-mid-cap-fund-direct-growth","title":"HDFC Large & Mid Cap Fund"},
      {"entity_type":"Scheme","scheme_code":null,"search_id":null,"title":"broken"}
    ]}"#;

    // Trimmed from a real page: two equities, a cash line, a bond, and a
    // row that is missing its share.
    const PAGE: &str = r#"{"scheme_name":"HDFC Mid Cap Fund Direct Growth","scheme_code":"118989",
      "direct_scheme_code":"118989","regular_scheme_code":null,"isin":"INF179K01XQ0","regular_isin":null,
      "aum":108324.5467,"holdings":[
        ["118989","2026-08-31","Repo","CASH","Unspecified","Repo",null,"7484.03","6.91",null,null,null],
        ["118989","2026-08-31","The Federal Bank Ltd","EQUITY","Financial","Equity",null,"4559.52","4.21",null,null,"the-federal-bank-ltd"],
        ["118989","2026-08-31","AU Small Finance Bank Ltd","EQUITY","Financial","Equity",null,"4316.27","3.98",null,null,"au-small-finance-bank-ltd"],
        ["118989","2026-08-31","GOVERNMENT OF INDIA 182 DAYS TBILL 22OT26","DEBT","Financial","Treasury Bills",null,"4.96","0.02",null,null,null],
        ["118989","2026-08-31","No Share Given","EQUITY","Financial","Equity",null,"1.00",null,null,null,null]
      ]}"#;

    const UNKNOWN_PAGE: &str = r#"{"scheme_name":null,"scheme_code":null,"isin":null,"holdings":null}"#;

    fn fund(code: Option<&str>, isin: Option<&str>) -> instrument::Model {
        let now = chrono::Utc::now().naive_utc();
        instrument::Model {
            id: 1,
            kind: "MUTUAL_FUND".into(),
            symbol: isin.unwrap_or("HDFCMID").into(),
            name: "HDFC Mid Cap Dir-G".into(),
            isin: isin.map(Into::into),
            exchange_id: None,
            currency_code: "INR".into(),
            country: Some("IN".into()),
            quote_symbol: None,
            amfi_scheme_code: code.map(Into::into),
            sector: None,
            history_from: None,
            holdings_ref: None,
            is_active: true,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn search_hits_keep_their_scheme_code_and_drop_broken_rows() {
        let hits = parse_search(SEARCH);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].search_id, "hdfc-mid-cap-fund-direct-growth");
        assert_eq!(hits[0].scheme_code.as_deref(), Some("118989"));
        assert!(hits[0].matches_code(&fund(Some("118989"), None)));
        assert!(!hits[1].matches_code(&fund(Some("118989"), None)));
        // Neither side having a code is not a match.
        assert!(!parse_search(r#"{"content":[{"search_id":"x"}]}"#)[0].matches_code(&fund(None, None)));
        assert!(parse_search("<html>blocked</html>").is_empty());
    }

    #[test]
    fn the_page_is_read_by_position_and_the_share_is_required() {
        let page = parse_page(PAGE).unwrap();
        assert_eq!(page.scheme_code.as_deref(), Some("118989"));
        assert_eq!(page.isin.as_deref(), Some("INF179K01XQ0"));
        assert_eq!(page.holdings.len(), 4, "the row without a share is dropped");

        let repo = &page.holdings[0];
        assert_eq!(repo.as_of, NaiveDate::from_ymd_opt(2026, 8, 31));
        assert_eq!(repo.holding.nature, "CASH");
        assert_eq!(repo.holding.sector, None, "Unspecified means none");
        assert_eq!(repo.holding.security_ref, None);

        let federal = &page.holdings[1].holding;
        assert_eq!(federal.name, "The Federal Bank Ltd");
        assert_eq!(federal.weight_pct, 4.21);
        assert_eq!(federal.sector.as_deref(), Some("Financial"));
        assert_eq!(federal.instrument_type.as_deref(), Some("Equity"));
        assert_eq!(federal.security_ref.as_deref(), Some("the-federal-bank-ltd"));
        assert_eq!(federal.market_value, Some(4559.52 * CRORE));
    }

    #[test]
    fn identity_needs_the_code_or_an_isin_to_agree() {
        let page = parse_page(PAGE).unwrap();
        assert!(is_same_fund(&page, &fund(Some("118989"), None)));
        assert!(is_same_fund(&page, &fund(None, Some("INF179K01XQ0"))), "by ISIN");
        assert!(is_same_fund(&page, &fund(None, Some("inf179k01xq0"))), "case-insensitively");
        assert!(!is_same_fund(&page, &fund(Some("130498"), None)), "a sibling scheme's code");
        assert!(!is_same_fund(&page, &fund(None, None)), "nothing to compare is no match");
        // The statement's ISIN is often the symbol, with the isin column empty.
        let mut by_symbol = fund(None, None);
        by_symbol.symbol = "INF179K01XQ0".into();
        assert!(is_same_fund(&page, &by_symbol));
    }

    #[test]
    fn an_unknown_slug_yields_an_empty_page_which_is_an_error_not_a_portfolio() {
        let page = parse_page(UNKNOWN_PAGE).unwrap();
        assert!(page.holdings.is_empty());
        assert!(!is_same_fund(&page, &fund(Some("118989"), Some("INF179K01XQ0"))));
        assert!(to_portfolio(&page, "no-such-fund").is_err());
        assert!(parse_page("not json").is_err());
    }

    #[test]
    fn the_portfolio_is_dated_by_its_rows() {
        let page = parse_page(PAGE).unwrap();
        let p = to_portfolio(&page, "hdfc-mid-cap-fund-direct-growth").unwrap();
        assert_eq!(p.as_of, NaiveDate::from_ymd_opt(2026, 8, 31).unwrap());
        assert_eq!(p.fund_ref, "hdfc-mid-cap-fund-direct-growth");
        assert_eq!(p.holdings.len(), 4);
    }

    #[test]
    fn references_are_url_slugs() {
        assert!(is_slug("hdfc-mid-cap-fund-direct-growth"));
        assert!(!is_slug("hdfc mid cap"));
        assert!(!is_slug("../other"));
        assert!(!is_slug(""));
    }
}
