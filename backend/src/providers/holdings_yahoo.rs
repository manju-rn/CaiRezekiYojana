//! ETF portfolios, via Yahoo Finance.
//!
//! Yahoo's quoteSummary endpoint gives an ETF's ten largest holdings and
//! its full sector split, for US, Australian and Singapore listings alike.
//! Ten names are not the whole portfolio, so what they leave out is stored
//! as one "not itemised" line per sector: the sector totals are then the
//! fund's own, and only the names inside them are partial.
//!
//! The endpoint wants a session cookie and a matching "crumb" token, both
//! fetched once and reused. It is unofficial, so it sits behind the
//! `HoldingsProvider` trait and a failure costs a refresh, never the
//! portfolio already stored.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::header::{COOKIE, SET_COOKIE, USER_AGENT};
use reqwest::StatusCode;
use sea_orm::EntityTrait;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::entities::{exchange, instrument};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

use super::quotes_yahoo::{provider_symbol, urlencode};
use super::{FundHolding, FundPortfolio, HoldingsProvider, REMAINDER_TYPE};

const PROVIDER: &str = "yahoo";
const COOKIE_URL: &str = "https://fc.yahoo.com/";
const CRUMB_URL: &str = "https://query2.finance.yahoo.com/v1/test/getcrumb";
const SUMMARY_URL: &str = "https://query2.finance.yahoo.com/v10/finance/quoteSummary";

/// The crumb endpoint turns away clients that do not look like a browser.
const BROWSER_UA: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

/// How long a cookie and crumb are reused before asking for new ones.
const SESSION_TTL: Duration = Duration::from_secs(30 * 60);

/// Between requests. Yahoo rate-limits bursts from one address hard.
const PACE: Duration = Duration::from_millis(300);

/// Lines below this share of the fund, in percent, are not worth a row.
const MIN_WEIGHT: f64 = 0.005;

pub struct YahooHoldingsProvider;

#[async_trait]
impl HoldingsProvider for YahooHoldingsProvider {
    fn id(&self) -> &'static str {
        PROVIDER
    }

    async fn portfolio(
        &self,
        state: &AppState,
        fund: &instrument::Model,
    ) -> AppResult<Option<FundPortfolio>> {
        // A reference on the instrument, set by hand, points at another
        // listing of the same fund.
        let symbol = match fund
            .holdings_ref
            .as_deref()
            .map(str::trim)
            .filter(|r| !r.is_empty())
        {
            Some(r) => r.to_string(),
            None => {
                let exchanges: HashMap<i32, exchange::Model> = exchange::Entity::find()
                    .all(&state.db)
                    .await?
                    .into_iter()
                    .map(|e| (e.id, e))
                    .collect();
                provider_symbol(fund, &exchanges)
            }
        };

        let Some(result) = summary(state, &symbol, "topHoldings").await? else {
            return Ok(None);
        };
        let Some(top) = parse_top_holdings(&result) else {
            return Ok(None);
        };
        if top.holdings.is_empty() && top.sectors.is_empty() {
            return Err(AppError::provider(
                PROVIDER,
                format!("{symbol}: no holdings published"),
            ));
        }

        // Each named holding's sector, so it can sit inside its sector's bar.
        // A failure here is not worth the whole fund: the holding is kept
        // without a sector and the sector remainders absorb the gap.
        let mut profiles = HashMap::new();
        for sym in top.holdings.iter().filter_map(|h| h.symbol.as_deref()) {
            match profile(state, sym).await {
                Ok(Some(p)) => {
                    profiles.insert(sym.to_string(), p);
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!(symbol = sym, error = %e, "holding profile lookup failed");
                    break;
                }
            }
        }

        Ok(Some(FundPortfolio {
            // Yahoo does not date the portfolio; it is the one current today.
            as_of: chrono::Utc::now().date_naive(),
            fund_ref: symbol,
            holdings: to_holdings(&top, &profiles),
        }))
    }
}

/// What Yahoo says a security is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    /// `EQUITY`, `ETF`, `MUTUALFUND`, ...
    pub quote_type: Option<String>,
    pub sector: Option<String>,
}

/// Kept for the life of the process: a sector does not change between two
/// refreshes, and the same large caps sit in most ETFs.
fn profiles() -> &'static Mutex<HashMap<String, Profile>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Profile>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// What Yahoo files a symbol as, and its sector. Cached per process.
pub async fn profile(state: &AppState, symbol: &str) -> AppResult<Option<Profile>> {
    if let Some(p) = profiles().lock().await.get(symbol) {
        return Ok(Some(p.clone()));
    }
    let Some(r) = summary(state, symbol, "assetProfile,quoteType").await? else {
        return Ok(None);
    };
    let p = Profile {
        quote_type: text(&r["quoteType"]["quoteType"]),
        sector: text(&r["assetProfile"]["sector"]),
    };
    profiles().lock().await.insert(symbol.to_string(), p.clone());
    Ok(Some(p))
}

// ---------------------------------------------------------------------------
// Session
// ---------------------------------------------------------------------------

struct Session {
    cookie: String,
    crumb: String,
    at: Instant,
}

fn session_cell() -> &'static Mutex<Option<Session>> {
    static CELL: OnceLock<Mutex<Option<Session>>> = OnceLock::new();
    CELL.get_or_init(|| Mutex::new(None))
}

fn rate_limited() -> AppError {
    AppError::provider(
        PROVIDER,
        "rate-limited by Yahoo (HTTP 429) - try again in a few minutes",
    )
}

/// A cookie from fc.yahoo.com, then the crumb that goes with it.
async fn session(state: &AppState) -> AppResult<(String, String)> {
    let mut guard = session_cell().lock().await;
    if let Some(s) = guard.as_ref().filter(|s| s.at.elapsed() < SESSION_TTL) {
        return Ok((s.cookie.clone(), s.crumb.clone()));
    }

    // fc.yahoo.com answers 404, and sets the cookie on the way.
    let res = state
        .http
        .get(COOKIE_URL)
        .header(USER_AGENT, BROWSER_UA)
        .send()
        .await?;
    let cookie = res
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter_map(|v| v.split(';').next())
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    if cookie.is_empty() {
        return Err(AppError::provider(
            PROVIDER,
            "no session cookie from fc.yahoo.com",
        ));
    }

    let res = state
        .http
        .get(CRUMB_URL)
        .header(USER_AGENT, BROWSER_UA)
        .header(COOKIE, &cookie)
        .send()
        .await?;
    let status = res.status();
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(rate_limited());
    }
    let crumb = res.text().await?.trim().to_string();
    // A crumb is a short token; anything else is an error page.
    if !status.is_success() || crumb.is_empty() || crumb.len() > 64 || crumb.contains(char::is_whitespace) {
        return Err(AppError::provider(
            PROVIDER,
            format!("no crumb from Yahoo (HTTP {status})"),
        ));
    }

    *guard = Some(Session {
        cookie: cookie.clone(),
        crumb: crumb.clone(),
        at: Instant::now(),
    });
    Ok((cookie, crumb))
}

/// One quoteSummary result, or `None` for a symbol Yahoo does not know.
async fn summary(state: &AppState, symbol: &str, modules: &str) -> AppResult<Option<Value>> {
    for attempt in 0..2 {
        let (cookie, crumb) = session(state).await?;
        tokio::time::sleep(PACE).await;
        let res = state
            .http
            .get(format!("{SUMMARY_URL}/{}", urlencode(symbol)))
            .query(&[("modules", modules), ("crumb", crumb.as_str())])
            .header(USER_AGENT, BROWSER_UA)
            .header(COOKIE, &cookie)
            .send()
            .await?;
        match res.status() {
            s if s.is_success() => {
                let v: Value = res.json().await?;
                return Ok(v["quoteSummary"]["result"].get(0).cloned());
            }
            // The crumb went stale: start a new session once.
            StatusCode::UNAUTHORIZED if attempt == 0 => *session_cell().lock().await = None,
            StatusCode::NOT_FOUND => return Ok(None),
            StatusCode::TOO_MANY_REQUESTS => return Err(rate_limited()),
            s => return Err(AppError::provider(PROVIDER, format!("{symbol}: HTTP {s}"))),
        }
    }
    Err(AppError::provider(PROVIDER, "Yahoo rejected a fresh session"))
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// The `topHoldings` module, shares as fractions of the fund.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TopHoldings {
    pub stock: f64,
    pub bond: f64,
    pub cash: f64,
    /// Preferred, convertible and everything else.
    pub other: f64,
    /// Largest first, as Yahoo lists them.
    pub holdings: Vec<TopHolding>,
    /// Share of the *equity* part in each sector, by display name.
    pub sectors: Vec<(String, f64)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TopHolding {
    pub symbol: Option<String>,
    pub name: String,
    pub weight: f64,
}

pub fn parse_top_holdings(result: &Value) -> Option<TopHoldings> {
    let t = result.get("topHoldings")?;
    let raw = |v: &Value| v["raw"].as_f64().or_else(|| v.as_f64());
    let share = |key: &str| raw(&t[key]).unwrap_or(0.0);

    let holdings = t["holdings"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|h| {
                    let symbol = text(&h["symbol"]);
                    let name = text(&h["holdingName"]).or_else(|| symbol.clone())?;
                    Some(TopHolding {
                        symbol,
                        name,
                        weight: raw(&h["holdingPercent"])?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    // `[{"technology": {"raw": 0.38}}, {"energy": {...}}, ...]`
    let sectors = t["sectorWeightings"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_object)
                .flat_map(|o| o.iter().filter_map(|(k, v)| Some((sector_label(k), raw(v)?))))
                .filter(|(_, w)| *w > 0.0)
                .collect()
        })
        .unwrap_or_default();

    Some(TopHoldings {
        stock: share("stockPosition"),
        bond: share("bondPosition"),
        cash: share("cashPosition"),
        other: share("otherPosition") + share("preferredPosition") + share("convertiblePosition"),
        holdings,
        sectors,
    })
}

/// `consumer_cyclical` -> `Consumer Cyclical`: the names Yahoo gives a
/// single stock's sector, so fund lines and direct holdings agree.
fn sector_label(key: &str) -> String {
    match key {
        "realestate" => "Real Estate".to_string(),
        "healthcare" => "Healthcare".to_string(),
        other => other
            .split('_')
            .map(|w| {
                let mut c = w.chars();
                c.next()
                    .map(|f| f.to_uppercase().chain(c).collect::<String>())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// The named holdings, then one line for each part they leave out.
///
/// A sector's line is what the sector weighting says the fund holds there,
/// less the named stocks in it. The lines are then scaled together to what
/// the stock share leaves after the named stocks, which absorbs a holding
/// whose sector could not be looked up and Yahoo's rounding alike. Nothing
/// is allowed past the whole fund: a fund of funds names funds whose
/// contents Yahoo already counts in its stock and bond shares.
pub fn to_holdings(top: &TopHoldings, profiles: &HashMap<String, Profile>) -> Vec<FundHolding> {
    let bond_fund = top.bond > top.stock;
    let mut named = Vec::with_capacity(top.holdings.len());
    let mut named_in_sector: HashMap<String, f64> = HashMap::new();
    let (mut named_equity, mut named_debt, mut named_other) = (0.0, 0.0, 0.0);

    for h in &top.holdings {
        let profile = h.symbol.as_ref().and_then(|s| profiles.get(s));
        let nature = match profile.and_then(|p| p.quote_type.as_deref()) {
            Some("EQUITY") => "EQUITY",
            Some("ETF" | "MUTUALFUND") => "MF",
            _ if bond_fund => "DEBT",
            _ => "EQUITY",
        };
        let pct = h.weight * 100.0;
        let sector = if nature == "EQUITY" {
            profile.and_then(|p| p.sector.clone())
        } else {
            None
        };
        match nature {
            "EQUITY" => {
                named_equity += pct;
                if let Some(s) = &sector {
                    *named_in_sector.entry(s.clone()).or_default() += pct;
                }
            }
            "DEBT" => named_debt += pct,
            _ => named_other += pct,
        }
        let kind = match nature {
            "EQUITY" => "Equity",
            "MF" => "Fund",
            _ => "Bond",
        };
        named.push(line(&h.name, nature, kind, sector, pct, h.symbol.clone()));
    }

    let mut rest = Vec::new();
    let equity_left = (top.stock * 100.0 - named_equity).max(0.0);
    let sector_total: f64 = top.sectors.iter().map(|(_, w)| w).sum();
    let by_sector: Vec<(String, f64)> = top
        .sectors
        .iter()
        .map(|(s, w)| {
            let expected = top.stock * 100.0 * w / sector_total;
            (
                s.clone(),
                (expected - named_in_sector.get(s).copied().unwrap_or(0.0)).max(0.0),
            )
        })
        .collect();
    let by_sector_total: f64 = by_sector.iter().map(|(_, w)| w).sum();
    if by_sector_total > 0.0 {
        for (s, w) in by_sector {
            let name = format!("Other {s} holdings");
            rest.push(line(
                &name,
                "EQUITY",
                REMAINDER_TYPE,
                Some(s),
                w * equity_left / by_sector_total,
                None,
            ));
        }
    } else {
        rest.push(line(
            "Other stocks",
            "EQUITY",
            REMAINDER_TYPE,
            None,
            equity_left,
            None,
        ));
    }
    rest.push(line(
        "Other bonds",
        "DEBT",
        REMAINDER_TYPE,
        None,
        top.bond * 100.0 - named_debt,
        None,
    ));
    rest.push(line("Cash", "CASH", "Cash", None, top.cash * 100.0, None));
    rest.push(line(
        "Other holdings",
        "OTHER",
        REMAINDER_TYPE,
        None,
        top.other * 100.0 - named_other,
        None,
    ));
    rest.retain(|h| h.weight_pct >= MIN_WEIGHT);

    let positions = (top.stock + top.bond + top.cash + top.other) * 100.0;
    let whole = if positions > 0.0 { positions } else { 100.0 };
    let named_total: f64 = named.iter().map(|h| h.weight_pct).sum();
    let rest_total: f64 = rest.iter().map(|h| h.weight_pct).sum();
    if rest_total > 0.0 && named_total + rest_total > whole + MIN_WEIGHT {
        let scale = (whole - named_total).max(0.0) / rest_total;
        for h in rest.iter_mut() {
            h.weight_pct *= scale;
        }
        rest.retain(|h| h.weight_pct >= MIN_WEIGHT);
    }

    named.extend(rest);
    named
}

fn line(
    name: &str,
    nature: &str,
    kind: &str,
    sector: Option<String>,
    weight_pct: f64,
    security_ref: Option<String>,
) -> FundHolding {
    FundHolding {
        name: name.to_string(),
        nature: nature.to_string(),
        instrument_type: Some(kind.to_string()),
        sector,
        weight_pct,
        market_value: None,
        security_ref,
    }
}

fn text(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Trimmed from a real VOO response: three of the ten names and five of
    // the eleven sectors, so the shares no longer sum to one on purpose.
    const VOO: &str = r#"{"topHoldings":{"cashPosition":{"raw":7.0E-4},"stockPosition":{"raw":0.9987},
      "bondPosition":{"raw":0.0},"otherPosition":{"raw":5.9999997E-4},"preferredPosition":{"raw":0.0},
      "convertiblePosition":{"raw":0.0},"holdings":[
        {"symbol":"NVDA","holdingName":"NVIDIA Corp","holdingPercent":{"raw":0.0808197}},
        {"symbol":"AAPL","holdingName":"Apple Inc","holdingPercent":{"raw":0.0703389}},
        {"symbol":"JPM","holdingName":"JPMorgan Chase & Co","holdingPercent":{"raw":0.015}},
        {"holdingName":"No Share Given"}],
      "sectorWeightings":[{"technology":{"raw":0.40}},{"financial_services":{"raw":0.12}},
        {"healthcare":{"raw":0.09}},{"consumer_cyclical":{"raw":0.09}},{"realestate":{"raw":0.0}}]}}"#;

    fn equity(sector: &str) -> Profile {
        Profile {
            quote_type: Some("EQUITY".into()),
            sector: Some(sector.into()),
        }
    }

    fn weight(h: &[FundHolding], name: &str) -> f64 {
        h.iter()
            .find(|x| x.name == name)
            .map(|x| x.weight_pct)
            .unwrap_or(0.0)
    }

    #[test]
    fn the_module_is_read_as_fractions_with_sectors_named_as_for_a_stock() {
        let v: Value = serde_json::from_str(VOO).unwrap();
        let top = parse_top_holdings(&v).unwrap();
        assert_eq!(top.holdings.len(), 3, "a holding with no share is dropped");
        assert_eq!(top.holdings[0].symbol.as_deref(), Some("NVDA"));
        assert!((top.stock - 0.9987).abs() < 1e-12);
        let names: Vec<&str> = top.sectors.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(
            names,
            [
                "Technology",
                "Financial Services",
                "Healthcare",
                "Consumer Cyclical"
            ]
        );
        assert!(parse_top_holdings(&serde_json::json!({"quoteType": {}})).is_none());
    }

    #[test]
    fn what_the_named_holdings_leave_is_spread_over_the_sectors_and_sums_to_the_fund() {
        let v: Value = serde_json::from_str(VOO).unwrap();
        let top = parse_top_holdings(&v).unwrap();
        let mut profiles = HashMap::new();
        profiles.insert("NVDA".to_string(), equity("Technology"));
        profiles.insert("AAPL".to_string(), equity("Technology"));
        // JPM's lookup failed: it stays, without a sector.

        let h = to_holdings(&top, &profiles);
        assert_eq!(h[0].name, "NVIDIA Corp");
        assert_eq!(h[0].sector.as_deref(), Some("Technology"));
        assert_eq!(h[0].security_ref.as_deref(), Some("NVDA"));
        assert_eq!(h[2].sector, None);

        let total: f64 = h.iter().map(|x| x.weight_pct).sum();
        // Yahoo's own shares carry float32 noise (otherPosition is 5.9999997E-4).
        assert!(
            (total - 100.0).abs() < 1e-6,
            "the lines add up to the whole fund, got {total}"
        );

        let tech_named = 8.08197 + 7.03389;
        let tech_rest = weight(&h, "Other Technology holdings");
        let fin_rest = weight(&h, "Other Financial Services holdings");
        // Before the rescale, tech's rest is its sector share less its named stocks.
        let raw_tech = 99.87 * 0.40 / 0.70 - tech_named;
        let raw_fin = 99.87 * 0.12 / 0.70;
        assert!(
            (tech_rest / fin_rest - raw_tech / raw_fin).abs() < 1e-9,
            "scaled together"
        );
        assert!(h
            .iter()
            .all(|x| x.instrument_type.as_deref() == Some(REMAINDER_TYPE)
                || x.security_ref.is_some()
                || x.nature == "CASH"));
        assert!(
            weight(&h, "Other bonds") == 0.0,
            "an empty bond share gets no line"
        );
        assert!((weight(&h, "Cash") - 0.07).abs() < 1e-9);
    }

    #[test]
    fn a_bond_fund_names_bonds_and_a_fund_of_funds_never_passes_the_whole() {
        let bonds = TopHoldings {
            stock: 0.0,
            bond: 0.98,
            cash: 0.02,
            other: 0.0,
            holdings: vec![TopHolding {
                symbol: None,
                name: "US Treasury 4%".into(),
                weight: 0.03,
            }],
            sectors: vec![],
        };
        let h = to_holdings(&bonds, &HashMap::new());
        assert_eq!(h[0].nature, "DEBT");
        assert!((weight(&h, "Other bonds") - 95.0).abs() < 1e-9);

        let fof = TopHoldings {
            stock: 0.6,
            bond: 0.4,
            cash: 0.0,
            other: 0.0,
            holdings: vec![
                TopHolding {
                    symbol: Some("IVV".into()),
                    name: "iShares Core S&P 500".into(),
                    weight: 0.6,
                },
                TopHolding {
                    symbol: Some("IUSB".into()),
                    name: "iShares Core Total USD Bond".into(),
                    weight: 0.4,
                },
            ],
            sectors: vec![("Technology".into(), 1.0)],
        };
        let mut profiles = HashMap::new();
        for s in ["IVV", "IUSB"] {
            profiles.insert(
                s.to_string(),
                Profile {
                    quote_type: Some("ETF".into()),
                    sector: None,
                },
            );
        }
        let h = to_holdings(&fof, &profiles);
        assert_eq!(h.len(), 2, "the named funds already are the whole fund");
        assert!(h.iter().all(|x| x.nature == "MF"));
    }

    #[test]
    fn sector_keys_become_names() {
        assert_eq!(sector_label("consumer_cyclical"), "Consumer Cyclical");
        assert_eq!(sector_label("realestate"), "Real Estate");
        assert_eq!(sector_label("communication_services"), "Communication Services");
    }
}
