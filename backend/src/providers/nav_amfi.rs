//! Indian mutual-fund NAV from AMFI.
//!
//! AMFI publishes every scheme's NAV as one semicolon-delimited text file,
//! refreshed each business evening. It is free, official, and has no key -
//! which makes it the right source for Indian funds.

use async_trait::async_trait;
use chrono::NaiveDate;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

use super::{Nav, NavProvider};

const AMFI_URL: &str = "https://www.amfiindia.com/spages/NAVAll.txt";
const PROVIDER: &str = "amfi";

pub struct AmfiProvider;

#[async_trait]
impl NavProvider for AmfiProvider {
    fn id(&self) -> &'static str {
        PROVIDER
    }

    async fn all_navs(&self, state: &AppState) -> AppResult<Vec<Nav>> {
        let url = state
            .setting_opt("amfi_url")
            .await
            .unwrap_or_else(|| AMFI_URL.to_string());

        let res = state.http.get(&url).send().await?;
        if !res.status().is_success() {
            return Err(AppError::provider(PROVIDER, format!("HTTP {}", res.status())));
        }
        let body = res.text().await?;
        let navs = parse_navall(&body);

        if navs.is_empty() {
            return Err(AppError::provider(PROVIDER, "no NAV rows parsed - upstream format may have changed"));
        }
        Ok(navs)
    }
}

/// Parse the NAVAll.txt body.
///
/// The file interleaves fund-house headings, category headings and blank
/// lines with data rows. Data rows are the semicolon-separated lines with a
/// numeric scheme code. A category heading looks like
/// `Open Ended Schemes ( Equity Scheme - Large Cap Fund )` and applies to
/// every data row until the next one; the bracketed part is carried along
/// as each row's `category`, which is what tells an equity fund from a debt
/// fund. AMFI's
/// column count has changed over time - originally
/// `Scheme Code;ISIN;ISIN Reinvest;Scheme Name;NAV;Date` (6 fields), now
/// with `Plan` and `Option` inserted before NAV/Date (8 fields). We locate
/// NAV/Date from the end of the row rather than a fixed index so either
/// layout (and any future one that only appends columns) still parses.
pub fn parse_navall(body: &str) -> Vec<Nav> {
    let mut out = Vec::new();
    let mut category: Option<String> = None;
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !line.contains(';') {
            if let Some(c) = parse_category_heading(line) {
                category = Some(c);
            }
            continue;
        }
        let parts: Vec<&str> = line.split(';').map(|p| p.trim()).collect();
        if parts.len() < 6 {
            continue;
        }
        // The header row and section titles fail this check.
        if parts[0].is_empty() || !parts[0].chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let nav_idx = parts.len() - 2;
        let date_idx = parts.len() - 1;
        let Ok(nav) = parts[nav_idx].parse::<f64>() else {
            // Suspended schemes publish "N.A." - skip rather than zero them.
            continue;
        };
        let Some(as_of) = parse_amfi_date(parts[date_idx]) else {
            continue;
        };
        out.push(Nav {
            scheme_code: parts[0].to_string(),
            scheme_name: parts[3].to_string(),
            isin: normalise_isin(parts[1]).or_else(|| normalise_isin(parts[2])),
            nav,
            as_of,
            category: category.clone(),
        });
    }
    out
}

/// `Open Ended Schemes(Debt Scheme - Money Market Fund)` ->
/// `Debt Scheme - Money Market Fund`. Fund-house lines carry no brackets and
/// yield `None`, which leaves the current category in force.
fn parse_category_heading(line: &str) -> Option<String> {
    let open = line.find('(')?;
    let close = line.rfind(')')?;
    if close <= open || !line[..open].to_ascii_lowercase().contains("scheme") {
        return None;
    }
    let inner = line[open + 1..close].trim();
    if inner.is_empty() {
        None
    } else {
        Some(inner.to_string())
    }
}

/// AMFI writes dates as `18-Sep-2026`.
fn parse_amfi_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%d-%b-%Y").ok()
}

/// Schemes without an ISIN for a given plan write `-` or `N.A.`.
fn normalise_isin(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() || s == "-" || s.eq_ignore_ascii_case("N.A.") {
        None
    } else {
        Some(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Current AMFI layout: Plan and Option columns inserted before NAV/Date.
    const SAMPLE: &str = "\
Scheme Code;ISIN Div Payout/ ISIN Growth;ISIN Div Reinvestment;Scheme Name;Plan;Option;Net Asset Value;Date

Aditya Birla Sun Life Mutual Fund
Open Ended Schemes ( Equity Scheme - Flexi Cap Fund )
119551;INF209K01157;INF209K01165;Aditya Birla Sun Life Flexi Cap Fund - Growth;Direct Plan;Growth Option;1543.2100;18-Sep-2026
119552;INF209K01173;-;Aditya Birla Sun Life Flexi Cap Fund - IDCW;Direct Plan;IDCW Option;180.4500;18-Sep-2026

HDFC Mutual Fund
Open Ended Schemes(Hybrid Scheme - Balanced Advantage Fund)
118989;INF179K01YV8;N.A.;HDFC Balanced Advantage Fund - Growth;Regular Plan;Growth Option;512.0330;18-Sep-2026
100000;INF000000000;-;Suspended Scheme;Regular Plan;Growth Option;N.A.;18-Sep-2026
";

    // The pre-2026 layout AMFI used before adding Plan/Option columns.
    const OLD_FORMAT_SAMPLE: &str = "\
Scheme Code;ISIN Div Payout/ISIN Growth;ISIN Div Reinvestment;Scheme Name;Net Asset Value;Date
119551;INF209K01157;INF209K01165;Aditya Birla Sun Life Flexi Cap Fund - Growth;1543.2100;18-Sep-2026
";

    #[test]
    fn parses_data_rows_and_ignores_headings() {
        let navs = parse_navall(SAMPLE);
        assert_eq!(navs.len(), 3, "the N.A. row and all headings are dropped");
        assert_eq!(navs[0].scheme_code, "119551");
        assert_eq!(navs[0].nav, 1543.21);
        assert_eq!(navs[0].isin.as_deref(), Some("INF209K01157"));
        assert_eq!(navs[0].as_of, NaiveDate::from_ymd_opt(2026, 9, 18).unwrap());
    }

    #[test]
    fn category_headings_carry_down_to_their_rows() {
        let navs = parse_navall(SAMPLE);
        assert_eq!(navs[0].category.as_deref(), Some("Equity Scheme - Flexi Cap Fund"));
        assert_eq!(navs[1].category.as_deref(), Some("Equity Scheme - Flexi Cap Fund"));
        assert_eq!(navs[2].category.as_deref(), Some("Hybrid Scheme - Balanced Advantage Fund"));
        assert!(parse_navall(OLD_FORMAT_SAMPLE)[0].category.is_none());
    }

    #[test]
    fn falls_back_to_the_reinvestment_isin_and_drops_placeholders() {
        let navs = parse_navall(SAMPLE);
        assert_eq!(navs[1].isin.as_deref(), Some("INF209K01173"));
        assert_eq!(navs[2].isin.as_deref(), Some("INF179K01YV8"));
    }

    #[test]
    fn parses_the_older_six_column_layout_too() {
        let navs = parse_navall(OLD_FORMAT_SAMPLE);
        assert_eq!(navs.len(), 1);
        assert_eq!(navs[0].nav, 1543.21);
        assert_eq!(navs[0].as_of, NaiveDate::from_ymd_opt(2026, 9, 18).unwrap());
    }

    #[test]
    fn a_garbage_body_yields_nothing_rather_than_panicking() {
        assert!(parse_navall("").is_empty());
        assert!(parse_navall("<html><body>maintenance</body></html>").is_empty());
    }
}
