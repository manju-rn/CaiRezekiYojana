//! Indian mutual-fund scheme names and categories.
//!
//! Two jobs live here, both pure string work:
//!
//! 1. **Matching a statement's scheme name to an AMFI row.** Registrar and
//!    portal exports (Value Research's transaction history in particular)
//!    carry no AMFI scheme code, and abbreviate the name: `SBI Large Cap
//!    Dir-G` where AMFI says `SBI Large Cap Fund - Direct Plan - Growth`.
//!    Without a code the fund never gets a NAV, so its chart is a handful of
//!    purchase-date dots and its XIRR is measured against its own cost.
//!    [`match_by_name`] closes that gap conservatively: every meaningful word
//!    of the statement name must appear in the AMFI name, plan and option
//!    must agree, and a tie between two candidates is treated as no match.
//!
//! 2. **Classifying a fund as Equity / Debt / Hybrid** and into its SEBI
//!    sub-category, from the AMFI section heading (`Equity Scheme - Large
//!    Cap Fund`) when we have it, or from the name when we do not.

use std::collections::BTreeSet;

/// Plan and option, as far as a name reveals them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Plan {
    Direct,
    Regular,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Payout {
    Growth,
    Idcw,
    Unknown,
}

/// A scheme name reduced to the words that identify the fund.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameKey {
    words: BTreeSet<String>,
    plan: Plan,
    payout: Payout,
}

/// Words that appear in almost every scheme name and carry no identity.
const NOISE: &[&str] = &[
    "fund", "funds", "plan", "option", "scheme", "the", "of", "an", "a", "mutual", "mf",
    "payout", "reinvestment", "reinvest", "formerly", "known", "as", "erstwhile", "nav",
];

/// Compound words that exports write with and without a space.
const COMPOUNDS: &[(&str, &str, &str)] = &[
    ("large", "cap", "largecap"),
    ("mid", "cap", "midcap"),
    ("small", "cap", "smallcap"),
    ("flexi", "cap", "flexicap"),
    ("multi", "cap", "multicap"),
    ("micro", "cap", "microcap"),
    ("blue", "chip", "bluechip"),
    ("top", "100", "top100"),
    ("top", "200", "top200"),
    ("g", "sec", "gsec"),
];

/// Abbreviations portals use, and the AMFI words they stand for. `None`
/// means the word is fine as it is.
fn expand(word: &str) -> Option<&'static [&'static str]> {
    let full: &'static [&'static str] = match word {
        "absl" => &["aditya", "birla", "sun", "life"],
        "pru" => &["prudential"],
        "yr" | "yrs" | "years" => &["year"],
        "govt" => &["government"],
        "opp" | "opps" => &["opportunities"],
        "intl" => &["international"],
        "inv" => &["investment"],
        _ => return None,
    };
    Some(full)
}

/// Split a name into lowercase alphanumeric words, breaking letter/digit
/// boundaries so `nifty50` and `nifty 50` agree.
fn words(name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut last_digit: Option<bool> = None;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            let digit = c.is_ascii_digit();
            if last_digit.is_some_and(|d| d != digit) && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            cur.push(c.to_ascii_lowercase());
            last_digit = Some(digit);
        } else {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            last_digit = None;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

impl NameKey {
    pub fn parse(name: &str) -> Self {
        let raw = words(name);

        // Merge "large cap" -> "largecap" (and "g sec" -> "gsec") first, so
        // both spellings agree and a "G" that is half of "G-Sec" is never
        // mistaken for the Growth option below.
        let mut merged: Vec<String> = Vec::with_capacity(raw.len());
        let mut i = 0;
        while i < raw.len() {
            if i + 1 < raw.len() {
                if let Some((_, _, joined)) =
                    COMPOUNDS.iter().find(|(a, b, _)| raw[i] == *a && raw[i + 1] == *b)
                {
                    merged.push(joined.to_string());
                    i += 2;
                    continue;
                }
            }
            merged.push(raw[i].clone());
            i += 1;
        }

        let mut plan = Plan::Unknown;
        let mut payout = Payout::Unknown;
        let mut words: BTreeSet<String> = BTreeSet::new();
        for w in merged {
            match w.as_str() {
                "dir" | "direct" => plan = Plan::Direct,
                "reg" | "regular" => plan = Plan::Regular,
                // Value Research: `Dir-G`; AMFI: `Growth`.
                "g" | "gr" | "growth" => payout = Payout::Growth,
                "idcw" | "div" | "dividend" | "d" | "bonus" => payout = Payout::Idcw,
                _ if NOISE.contains(&w.as_str()) => {}
                _ => match expand(&w) {
                    Some(full) => words.extend(full.iter().map(|s| s.to_string())),
                    None => {
                        words.insert(w.clone());
                    }
                },
            }
        }

        Self { words, plan, payout }
    }

    fn compatible_with(&self, other: &NameKey) -> bool {
        // AMFI spells out "Direct" and often omits "Regular"; statements
        // that name no plan are, in practice, the regular plan too.
        let plan = |p: Plan| if p == Plan::Unknown { Plan::Regular } else { p };
        let plan_ok = plan(self.plan) == plan(other.plan);
        // An AMFI row that says neither Growth nor IDCW is most often Growth;
        // let the scoring prefer an explicit match rather than refusing here.
        let payout_ok = match (self.payout, other.payout) {
            (Payout::Unknown, _) | (_, Payout::Unknown) => true,
            (a, b) => a == b,
        };
        plan_ok && payout_ok
    }
}

/// Anything the matcher can pick from: a scheme code plus its full name.
pub trait Candidate {
    fn scheme_name(&self) -> &str;
}

impl Candidate for (String, String) {
    fn scheme_name(&self) -> &str {
        &self.1
    }
}

/// How many surplus words an AMFI name may carry beyond the statement name.
/// "(Formerly SBI Blue Chip Fund)" costs one; a wholly different fund costs
/// many more.
const MAX_EXTRA_WORDS: usize = 3;

/// Find the one candidate the statement `name` describes, or `None` when
/// nothing fits or more than one fits equally well.
pub fn match_by_name<'a, C: Candidate>(name: &str, candidates: &'a [C]) -> Option<&'a C> {
    let target = NameKey::parse(name);
    if target.words.len() < 2 {
        // "Growth" alone, or a single word, describes hundreds of schemes.
        return None;
    }

    let mut best: Option<(usize, bool, &C)> = None;
    let mut tied = false;

    for c in candidates {
        let key = NameKey::parse(c.scheme_name());
        if !target.compatible_with(&key) || !target.words.is_subset(&key.words) {
            continue;
        }
        let extra = key.words.len() - target.words.len();
        if extra > MAX_EXTRA_WORDS {
            continue;
        }
        // Prefer a candidate that states the payout the statement asked for
        // over one that leaves it implicit.
        let explicit = key.payout == target.payout && key.payout != Payout::Unknown;
        let score = (extra, !explicit);
        match best {
            None => {
                best = Some((extra, explicit, c));
                tied = false;
            }
            Some((b_extra, b_explicit, _)) => {
                let b_score = (b_extra, !b_explicit);
                if score < b_score {
                    best = Some((extra, explicit, c));
                    tied = false;
                } else if score == b_score {
                    tied = true;
                }
            }
        }
    }

    match best {
        Some((_, _, c)) if !tied => Some(c),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// Top-level split the Indian mutual-fund tab reports on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FundClass {
    Equity,
    Debt,
    Hybrid,
    /// Gold, silver, solution-oriented and anything else that is neither.
    Other,
}

impl FundClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            FundClass::Equity => "Equity",
            FundClass::Debt => "Debt",
            FundClass::Hybrid => "Hybrid",
            FundClass::Other => "Other",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Classification {
    pub class: FundClass,
    /// SEBI sub-category, e.g. `Large Cap`, `Money Market`, `Index`.
    pub category: Option<String>,
}

/// Words in a name or category that mark a debt fund. Checked against the
/// lowercase text with spaces, so `money market` matches as a phrase.
const DEBT_MARKERS: &[&str] = &[
    "debt", "gilt", "g-sec", "gsec", "bond", "sdl", "treasury", "money market", "liquid",
    "overnight", "duration", "ultra short", "short term", "credit risk", "banking and psu",
    "banking & psu", "floater", "floating", "interest rate", "fixed maturity", "fmp",
    "target maturity", "crisil ibx", "constant maturity", "10 year", "10 yr",
];

const OTHER_MARKERS: &[&str] = &["gold", "silver", "commodit", "retirement", "children", "child"];

/// (phrase, sub-category) for names imported without an AMFI category.
const NAME_CATEGORIES: &[(&str, &str)] = &[
    ("money market", "Money Market"),
    ("overnight", "Overnight"),
    ("liquid", "Liquid"),
    ("ultra short", "Ultra Short Duration"),
    ("low duration", "Low Duration"),
    ("short duration", "Short Duration"),
    ("short term", "Short Duration"),
    ("medium duration", "Medium Duration"),
    ("long duration", "Long Duration"),
    ("dynamic bond", "Dynamic Bond"),
    ("corporate bond", "Corporate Bond"),
    ("credit risk", "Credit Risk"),
    ("banking and psu", "Banking and PSU"),
    ("banking & psu", "Banking and PSU"),
    ("gilt", "Gilt"),
    ("floater", "Floater"),
    ("floating", "Floater"),
    // Sectoral / thematic equity, which SEBI files together.
    ("psu equity", "Sectoral / Thematic"),
    ("thematic", "Sectoral / Thematic"),
    ("sectoral", "Sectoral / Thematic"),
    ("infrastructure", "Sectoral / Thematic"),
    ("banking & financial", "Sectoral / Thematic"),
    ("banking and financial", "Sectoral / Thematic"),
    ("financial services", "Sectoral / Thematic"),
    ("pharma", "Sectoral / Thematic"),
    ("healthcare", "Sectoral / Thematic"),
    ("technology", "Sectoral / Thematic"),
    ("consumption", "Sectoral / Thematic"),
    ("manufacturing", "Sectoral / Thematic"),
    ("business cycle", "Sectoral / Thematic"),
    ("energy", "Sectoral / Thematic"),
    ("transportation", "Sectoral / Thematic"),
    ("digital", "Sectoral / Thematic"),
    ("esg", "Sectoral / Thematic"),
    ("mnc", "Sectoral / Thematic"),
    ("target maturity", "Target Maturity"),
    ("large and mid", "Large & Mid Cap"),
    ("large & mid", "Large & Mid Cap"),
    ("largecap", "Large Cap"),
    ("large cap", "Large Cap"),
    ("bluechip", "Large Cap"),
    ("blue chip", "Large Cap"),
    ("midcap", "Mid Cap"),
    ("mid cap", "Mid Cap"),
    ("smallcap", "Small Cap"),
    ("small cap", "Small Cap"),
    ("flexicap", "Flexi Cap"),
    ("flexi cap", "Flexi Cap"),
    ("multicap", "Multi Cap"),
    ("multi cap", "Multi Cap"),
    ("elss", "ELSS"),
    ("tax saver", "ELSS"),
    ("tax saving", "ELSS"),
    ("focused", "Focused"),
    ("contra", "Contra"),
    ("value", "Value"),
    ("dividend yield", "Dividend Yield"),
    ("balanced advantage", "Balanced Advantage"),
    ("dynamic asset", "Balanced Advantage"),
    ("aggressive hybrid", "Aggressive Hybrid"),
    ("equity savings", "Equity Savings"),
    ("arbitrage", "Arbitrage"),
    ("multi asset", "Multi Asset"),
    ("index", "Index"),
    ("etf", "ETF"),
    ("nifty", "Index"),
    ("sensex", "Index"),
    ("fof", "Fund of Funds"),
    ("fund of fund", "Fund of Funds"),
    ("gold", "Gold"),
    ("silver", "Silver"),
];

fn contains_any(text: &str, markers: &[&str]) -> bool {
    markers.iter().any(|m| text.contains(*m))
}

/// Tidy an AMFI sub-category: `Large Cap Fund` -> `Large Cap`,
/// `Index Funds` -> `Index`, `ETFs` -> `ETF`.
fn tidy_category(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    for suffix in [" Funds", " Fund", " funds", " fund"] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            s = stripped.to_string();
            break;
        }
    }
    if s.eq_ignore_ascii_case("etfs") {
        s = "ETF".to_string();
    }
    if s.is_empty() {
        raw.trim().to_string()
    } else {
        s
    }
}

/// Classify from the AMFI category heading when known, else from the name.
///
/// AMFI files index funds, ETFs and FoFs under "Other Scheme", so for those
/// the name decides whether the underlying index is equity or debt - a Nifty
/// 50 index fund and a 10-year gilt index fund are both "Other" to AMFI but
/// sit on opposite sides of the split this app draws.
pub fn classify(category: Option<&str>, name: &str) -> Classification {
    let lname = name.to_lowercase();

    if let Some(cat) = category.map(str::trim).filter(|c| !c.is_empty()) {
        let (head, sub) = match cat.split_once(" - ") {
            Some((h, s)) => (h.trim().to_lowercase(), Some(s.trim())),
            None => (cat.to_lowercase(), None),
        };
        let sub_l = sub.map(|s| s.to_lowercase()).unwrap_or_default();

        let class = if head.starts_with("equity") {
            FundClass::Equity
        } else if head.starts_with("debt") {
            FundClass::Debt
        } else if head.starts_with("hybrid") {
            FundClass::Hybrid
        } else if head.starts_with("solution") {
            FundClass::Other
        } else if contains_any(&lname, DEBT_MARKERS) || contains_any(&sub_l, DEBT_MARKERS) {
            FundClass::Debt
        } else if contains_any(&lname, OTHER_MARKERS) || contains_any(&sub_l, OTHER_MARKERS) {
            FundClass::Other
        } else {
            // Index funds, ETFs and FoFs that are not debt track equity.
            FundClass::Equity
        };

        let category = sub.map(tidy_category).or_else(|| Some(tidy_category(cat)));
        return Classification { class, category };
    }

    // No AMFI heading yet - read the name.
    let class = if contains_any(&lname, DEBT_MARKERS) {
        FundClass::Debt
    } else if contains_any(&lname, &["hybrid", "balanced advantage", "equity savings", "arbitrage", "multi asset", "dynamic asset"]) {
        FundClass::Hybrid
    } else if contains_any(&lname, OTHER_MARKERS) {
        FundClass::Other
    } else {
        FundClass::Equity
    };
    let category = NAME_CATEGORIES
        .iter()
        .find(|(phrase, _)| lname.contains(*phrase))
        .map(|(_, cat)| cat.to_string());
    Classification { class, category }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn amfi(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter().map(|(c, n)| (c.to_string(), n.to_string())).collect()
    }

    #[test]
    fn value_research_abbreviations_match_the_amfi_row() {
        let rows = amfi(&[
            ("125497", "SBI Large Cap Fund - Direct Plan - Growth"),
            ("125494", "SBI Large Cap Fund - Regular Plan - Growth"),
            ("125495", "SBI Large Cap Fund - Direct Plan - IDCW"),
            ("125496", "SBI Large & Midcap Fund - Direct Plan - Growth"),
        ]);
        assert_eq!(match_by_name("SBI Large Cap Dir-G", &rows).map(|c| c.0.as_str()), Some("125497"));
        assert_eq!(match_by_name("SBI Large Cap Reg-G", &rows).map(|c| c.0.as_str()), Some("125494"));
        assert_eq!(match_by_name("SBI Large Cap Dir-IDCW", &rows).map(|c| c.0.as_str()), Some("125495"));
    }

    #[test]
    fn a_renamed_scheme_still_matches_through_its_formerly_clause() {
        let rows = amfi(&[(
            "1",
            "SBI Large Cap Fund (Formerly SBI Blue Chip Fund) - Direct Plan - Growth Option",
        )]);
        assert!(match_by_name("SBI Large Cap Dir-G", &rows).is_some());
    }

    #[test]
    fn compound_words_agree_either_way_they_are_spelt() {
        let rows = amfi(&[("1", "Motilal Oswal Mid Cap Fund - Direct Plan - Growth Option")]);
        assert!(match_by_name("Motilal Oswal Midcap Dir-G", &rows).is_some());
        let rows = amfi(&[("2", "UTI Nifty50 Index Fund - Direct Plan - Growth Option")]);
        assert!(match_by_name("UTI Nifty 50 Index Dir-G", &rows).is_some());
    }

    #[test]
    fn a_missing_word_is_not_a_match() {
        // "Large Cap" is not "Large & Mid Cap", however similar they read.
        let rows = amfi(&[("1", "Mirae Asset Large & Midcap Fund - Direct Plan - Growth")]);
        assert!(match_by_name("Mirae Asset Large Cap Dir-G", &rows).is_none());
    }

    #[test]
    fn plan_and_payout_must_agree() {
        let rows = amfi(&[("1", "PGIM India Money Market Fund - Direct Plan - Growth")]);
        assert!(match_by_name("PGIM India Money Market Reg-G", &rows).is_none());
        assert!(match_by_name("PGIM India Money Market Dir-IDCW", &rows).is_none());
        assert!(match_by_name("PGIM India Money Market Dir-G", &rows).is_some());
    }

    #[test]
    fn an_amfi_row_without_a_plan_word_is_the_regular_plan() {
        let rows = amfi(&[("1", "HDFC Balanced Advantage Fund - Growth Option")]);
        assert!(match_by_name("HDFC Balanced Advantage Reg-G", &rows).is_some());
        assert!(match_by_name("HDFC Balanced Advantage Dir-G", &rows).is_none());
    }

    #[test]
    fn ties_and_single_words_are_refused() {
        let rows = amfi(&[
            ("1", "Axis Bluechip Fund - Direct Plan - Growth"),
            ("2", "Axis Bluechip Fund - Direct Plan - Growth"),
        ]);
        assert!(match_by_name("Axis Bluechip Dir-G", &rows).is_none());
        assert!(match_by_name("Growth", &rows).is_none());
    }

    #[test]
    fn far_longer_amfi_names_do_not_count_as_the_same_fund() {
        let rows = amfi(&[(
            "1",
            "Kotak Nifty Alpha 50 Quality Low Volatility Momentum Index Fund - Direct Plan - Growth",
        )]);
        assert!(match_by_name("Kotak Nifty Dir-G", &rows).is_none());
    }

    #[test]
    fn amfi_headings_classify_directly() {
        let c = classify(Some("Equity Scheme - Large Cap Fund"), "SBI Large Cap Fund");
        assert_eq!(c.class, FundClass::Equity);
        assert_eq!(c.category.as_deref(), Some("Large Cap"));

        let c = classify(Some("Debt Scheme - Money Market Fund"), "SBI Money Market Fund");
        assert_eq!(c.class, FundClass::Debt);
        assert_eq!(c.category.as_deref(), Some("Money Market"));

        let c = classify(Some("Hybrid Scheme - Balanced Advantage Fund"), "x");
        assert_eq!(c.class, FundClass::Hybrid);
        assert_eq!(c.category.as_deref(), Some("Balanced Advantage"));
    }

    #[test]
    fn other_scheme_index_funds_split_on_what_they_track() {
        let eq = classify(Some("Other Scheme - Index Funds"), "UTI Nifty 50 Index Fund");
        assert_eq!(eq.class, FundClass::Equity);
        assert_eq!(eq.category.as_deref(), Some("Index"));

        let debt = classify(
            Some("Other Scheme - Index Funds"),
            "UTI Nifty 10 yr Constant Maturity Gilt Index Fund",
        );
        assert_eq!(debt.class, FundClass::Debt);

        let gold = classify(Some("Other Scheme - FoF Domestic"), "SBI Gold Fund");
        assert_eq!(gold.class, FundClass::Other);
        assert_eq!(classify(Some("Other Scheme - ETFs"), "Nippon ETF").category.as_deref(), Some("ETF"));
    }

    #[test]
    fn fund_house_abbreviations_expand_to_the_amfi_name() {
        let rows = amfi(&[(
            "1",
            "Aditya Birla Sun Life Large Cap Fund - Direct Plan - Growth",
        )]);
        assert!(match_by_name("ABSL Large Cap Dir-G", &rows).is_some());
        let rows = amfi(&[(
            "2",
            "ICICI Prudential Nifty 200 Momentum 30 Index Fund - Direct Plan - Growth",
        )]);
        assert!(match_by_name("ICICI Pru Nifty 200 Momentum 30 Index Dir-G", &rows).is_some());
    }

    #[test]
    fn short_term_and_floating_rate_funds_are_debt() {
        let c = classify(None, "HDFC Ultra Short to Short Term Dir-G");
        assert_eq!(c.class, FundClass::Debt);
        assert_eq!(c.category.as_deref(), Some("Ultra Short Duration"));

        let c = classify(None, "ABSL Floating Interest Rate Dir-G");
        assert_eq!(c.class, FundClass::Debt);
        assert_eq!(c.category.as_deref(), Some("Floater"));

        let c = classify(None, "SBI 10 year Constant Maturity Gilt Dir-G");
        assert_eq!(c.class, FundClass::Debt);
        assert_eq!(c.category.as_deref(), Some("Gilt"));
    }

    #[test]
    fn sector_and_theme_funds_are_equity() {
        let c = classify(None, "ABSL PSU Equity Dir-G");
        assert_eq!(c.class, FundClass::Equity);
        assert_eq!(c.category.as_deref(), Some("Sectoral / Thematic"));

        let c = classify(None, "ABSL Value Dir-G");
        assert_eq!(c.class, FundClass::Equity);
        assert_eq!(c.category.as_deref(), Some("Value"));

        let c = classify(None, "DSP Large & Mid Cap Dir-G");
        assert_eq!(c.category.as_deref(), Some("Large & Mid Cap"));
        let c = classify(None, "Mirae Asset Large & Midcap Dir-G");
        assert_eq!(c.category.as_deref(), Some("Large & Mid Cap"));
    }

    #[test]
    fn names_alone_still_give_a_reasonable_answer() {
        let c = classify(None, "PGIM India Low Duration Dir-G");
        assert_eq!(c.class, FundClass::Debt);
        assert_eq!(c.category.as_deref(), Some("Low Duration"));

        let c = classify(None, "Motilal Oswal Midcap Dir-G");
        assert_eq!(c.class, FundClass::Equity);
        assert_eq!(c.category.as_deref(), Some("Mid Cap"));

        let c = classify(None, "Some Unheard Of Scheme");
        assert_eq!(c.class, FundClass::Equity);
        assert_eq!(c.category, None);
    }
}

/// The fund's name as a search query: its identifying words in their
/// original order, portal abbreviations spelt out, and the plan and option
/// dropped. A portfolio is disclosed per scheme, so `Direct` and `Growth`
/// would only narrow a search that should find the scheme itself.
pub fn search_phrase(name: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for w in words(name) {
        match w.as_str() {
            "dir" | "direct" | "reg" | "regular" | "g" | "gr" | "growth" | "idcw" | "div"
            | "dividend" | "d" | "bonus" | "plan" | "option" | "payout" | "reinvestment"
            | "reinvest" => {}
            _ => match expand(&w) {
                Some(full) => out.extend(full.iter().map(|s| s.to_string())),
                None => out.push(w),
            },
        }
    }
    out.join(" ")
}

#[cfg(test)]
mod search_phrase_tests {
    use super::search_phrase;

    #[test]
    fn keeps_word_order_and_drops_plan_and_option() {
        assert_eq!(search_phrase("ABSL Money Market Dir-G"), "aditya birla sun life money market");
        assert_eq!(
            search_phrase("SBI 10 year Constant Maturity Gilt Dir-G"),
            "sbi 10 year constant maturity gilt"
        );
        assert_eq!(
            search_phrase("ICICI Pru Nifty Next 50 Index Dir-G"),
            "icici prudential nifty next 50 index"
        );
        assert_eq!(
            search_phrase("Mirae Asset Large & Midcap Fund - Direct Plan - Growth"),
            "mirae asset large midcap fund"
        );
        assert_eq!(search_phrase("Direct Growth"), "");
    }
}
