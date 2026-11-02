//! Retirement planning: the Retirement screen's simulation, on the server,
//! so the API - and the MCP tools drawn from it - can say how long the
//! corpus lasts.
//!
//! The arithmetic is [`crate::domain::retirement`]. This fills in whatever
//! the caller leaves out the way the screen does - the corpus and its split
//! by currency from the dashboard, inflation from the base currency's
//! history, each currency's drift and volatility from its own - and says in
//! the answer which it used.

use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::domain::money::{round_money, round_to};
use crate::domain::retirement::{self as model, Extra, Leg, Plan, RowStatus, Shifts, YearRow};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

use super::networth;

/// The screen's starting withdrawals, as a share of the corpus each year.
const DEFAULT_RATES: [f64; 4] = [3.0, 4.0, 5.0, 6.0];
pub(crate) const DEFAULT_RETURN: f64 = 8.0;
pub(crate) const FALLBACK_INFLATION: f64 = 5.0;
const MAX_SCENARIOS: usize = 10;
pub(crate) const MAX_EXTRAS: usize = 50;
pub(crate) const MAX_AMOUNT: f64 = 1e15;

/// The assumptions to run. Every field is optional: what is left out
/// follows the portfolio or history, as on the Retirement screen.
#[derive(Clone, Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RetirementInput {
    /// Starting corpus in the base currency. Omit for today's net worth.
    pub corpus: Option<f64>,
    /// Calendar year of the first withdrawal. Omit for this year.
    pub start_year: Option<i32>,
    /// Assumed yearly return, %, earned by each currency's part in its own
    /// currency. Default 8.
    pub return_pct: Option<f64>,
    /// Assumed yearly inflation, %. Omit for the base currency's ten-year
    /// average (5 where there is no series for it).
    pub inflation_pct: Option<f64>,
    /// The withdrawals to compare, one scenario each, in today's money - per
    /// month when `monthly`, otherwise per year. Each grows with inflation.
    /// Omit for 3, 4, 5 and 6% of the corpus a year. At most 10.
    pub withdrawals: Option<Vec<f64>>,
    /// `withdrawals` are monthly amounts. Default false.
    pub monthly: Option<bool>,
    /// Amounts on top of the withdrawals: income in (positive) or spends
    /// out (negative). At most 50.
    pub extras: Option<Vec<ExtraInput>>,
    /// Apply the rupee's ten-year trend against each currency; false holds
    /// every currency level. Default false.
    pub fx_trend: Option<bool>,
    /// By currency code: the rupee's assumed yearly change against it, %.
    /// -4 means a rupee buys 4% less of it each year. Every pair follows
    /// from the rupee against each side. Replaces the trend (or level) for
    /// that currency, including the base when it is not INR.
    pub inr_moves: Option<BTreeMap<String, f64>>,
    /// The rupee's yearly change, %, against currencies with no history.
    /// Omit for its average against the ones that have one (with
    /// `fx_trend`), else 0.
    pub inr_other_pct: Option<f64>,
    /// By currency code: replace the drift or volatility against the base
    /// outright. A drift here wins over `inr_moves`.
    pub fx_overrides: Option<BTreeMap<String, FxOverride>>,
    /// Include the year-by-year table in each scenario. Default true.
    pub include_rows: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtraInput {
    pub label: Option<String>,
    /// Positive comes in (a pension, a sale), negative goes out (a wedding),
    /// in the base currency and today's money when `indexed`.
    pub amount: f64,
    /// First calendar year it applies to.
    pub from_year: i32,
    /// Last calendar year, inclusive. Omit for a one-off in `from_year`.
    pub to_year: Option<i32>,
    /// Grows with inflation from today's money. Default true.
    pub indexed: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FxOverride {
    /// Assumed yearly change against the base, %: +2 means the currency
    /// buys 2% more of the base each year.
    pub drift_pct: Option<f64>,
    /// Yearly volatility against the base, %, for the currency-risk band.
    pub volatility_pct: Option<f64>,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct RetirementPlan {
    pub base_currency: String,
    /// The date of the portfolio figures used.
    pub as_of: NaiveDate,
    pub assumptions: Assumptions,
    /// One per withdrawal, in the order given.
    pub scenarios: Vec<Scenario>,
    pub warnings: Vec<String>,
}

/// What was assumed - the caller's figures, and what filled the rest.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct Assumptions {
    pub corpus: f64,
    /// `portfolio` (today's net worth) or `given`.
    pub corpus_source: String,
    pub start_year: i32,
    /// How much of `start_year` the first row covers, 0-1: what is left of
    /// this year when the plan starts this year, else 1. That row's
    /// withdrawal, recurring extras and growth are for that share only.
    pub first_year_fraction: f64,
    /// Years every scenario is run for.
    pub years: u32,
    pub return_pct: f64,
    pub inflation_pct: f64,
    /// `given`, `history` (the base currency's ten-year average) or `fallback`.
    pub inflation_source: String,
    /// The country whose inflation history was used, when it was.
    pub inflation_country: Option<String>,
    pub monthly: bool,
    pub fx_trend: bool,
    /// The rupee's yearly change, %, used against currencies with no history.
    pub inr_other_pct: f64,
    /// Calendar years the inflation and currency histories cover.
    pub reference_from: i32,
    pub reference_to: i32,
    /// The corpus by currency, held at these weights every year.
    pub legs: Vec<LegView>,
    pub extras: Vec<ExtraView>,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct LegView {
    pub currency: String,
    /// Share of the corpus, %.
    pub weight_pct: f64,
    /// Assumed yearly change against the base, %. Zero for the base itself.
    pub drift_pct: f64,
    /// Yearly volatility against the base, %.
    pub volatility_pct: f64,
    /// What it did against the base over the reference years; null where
    /// there is no series for the pair.
    pub history: Option<FxHistoryView>,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct FxHistoryView {
    /// The whole period, %: +20 means the currency bought 20% more base.
    pub total_change_pct: f64,
    /// Compound annual change, %.
    pub trend_pct: f64,
    /// Standard deviation of the yearly change, %.
    pub volatility_pct: f64,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct ExtraView {
    pub label: Option<String>,
    pub amount: f64,
    pub from_year: i32,
    pub to_year: i32,
    pub indexed: bool,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct Scenario {
    /// As given: per month when `monthly`, otherwise per year, today's money.
    pub withdrawal: f64,
    /// The first year's withdrawal in full.
    pub withdrawal_yearly: f64,
    /// The first year's withdrawal as a share of the corpus, %. Null for an
    /// empty corpus.
    pub withdrawal_rate_pct: Option<f64>,
    /// Calendar year the corpus first goes below zero; null when it lasts
    /// every year run.
    pub runs_out_in: Option<i32>,
    /// Full years of withdrawals it covers; `years` when it never runs out.
    pub years_lasted: u32,
    /// The balance after the last year run - below zero when it ran out.
    pub final_closing: f64,
    /// The same in today's money.
    pub final_closing_real: f64,
    /// The same plan drawn month by month.
    pub monthly_draw: MonthlyDraw,
    /// The spread currency moves alone could give. Null when the whole
    /// corpus is in the base currency, or no foreign part has volatility.
    pub currency_risk: Option<CurrencyRisk>,
    /// Year by year; empty when `include_rows` is false.
    pub rows: Vec<RowView>,
}

/// A systematic withdrawal plan on the same assumptions: each month a
/// twelfth of the year's withdrawal comes out and the rest stays invested,
/// so it lasts a little longer than the yearly model says.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct MonthlyDraw {
    /// Full monthly withdrawals paid before the corpus falls short; null
    /// when it pays every month run.
    pub months_lasted: Option<u32>,
    /// `months_lasted` in years, or `years` when it lasts.
    pub years_lasted: f64,
    /// What is left after the last month, when it lasts.
    pub closing: Option<f64>,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct CurrencyRisk {
    /// How many random paths were run.
    pub paths: usize,
    /// Share of paths still above zero after every year run, %.
    pub survival_pct: f64,
    /// The 10th, 50th and 90th percentile closing balance, one per year,
    /// in the order of `rows`.
    pub p10: Vec<f64>,
    pub p50: Vec<f64>,
    pub p90: Vec<f64>,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct RowView {
    pub year: i32,
    /// 1 for the first year of the plan.
    pub index: u32,
    pub opening: f64,
    pub withdrawal: f64,
    pub extras: f64,
    pub returns: f64,
    /// What currency moves added (or took) on top of the return.
    pub fx_effect: f64,
    pub closing: f64,
    /// The closing balance in today's money.
    pub closing_real: f64,
    /// `ok`, `low` (under two years of withdrawals left) or `negative`.
    pub status: String,
}

/// Run every withdrawal in `input` against the caller's portfolio.
pub async fn simulate(state: &AppState, owner: i32, input: RetirementInput) -> AppResult<RetirementPlan> {
    validate(&input)?;
    let nw = networth::build(state, owner).await?;
    let base = nw.base_currency.clone();
    let mut warnings = Vec::new();

    let (corpus, corpus_source) = match input.corpus {
        Some(c) => (c, "given"),
        None => {
            // The figure rests on the dashboard's, so its caveats do too.
            warnings.extend(nw.warnings.iter().cloned());
            (nw.total_base.round(), "portfolio")
        }
    };
    let today = chrono::Utc::now().date_naive();
    let start_year = input.start_year.unwrap_or_else(|| today.year());
    let first_year_fraction = model::first_year_fraction(start_year, today);
    let return_pct = input.return_pct.unwrap_or(DEFAULT_RETURN);
    let history = model::historical_inflation(&base);
    let (inflation_pct, inflation_source, inflation_country) = match (input.inflation_pct, history) {
        (Some(i), _) => (i, "given", None),
        (None, Some((country, avg))) => (avg, "history", Some(country.to_string())),
        (None, None) => {
            warnings.push(format!("no inflation history for {base}: assumed {FALLBACK_INFLATION}%"));
            (FALLBACK_INFLATION, "fallback", None)
        }
    };
    let monthly = input.monthly.unwrap_or(false);
    let per_year = if monthly { 12.0 } else { 1.0 };
    let withdrawals = input
        .withdrawals
        .clone()
        .unwrap_or_else(|| DEFAULT_RATES.iter().map(|r| roundish(corpus * r / 100.0 / per_year)).collect());
    let fx_trend = input.fx_trend.unwrap_or(false);
    let (legs, inr_other) = currency_legs(
        &nw.currency_exposure,
        &base,
        &FxInput {
            fx_trend,
            inr_moves: input.inr_moves.clone(),
            inr_other_pct: input.inr_other_pct,
            fx_overrides: input.fx_overrides.clone(),
        },
        &mut warnings,
    );
    let extras = extras_of(input.extras.clone());

    let include_rows = input.include_rows.unwrap_or(true);
    let foreign = legs.iter().any(|l| l.currency != base && l.volatility_pct > 0.0);
    let scenarios = withdrawals
        .iter()
        .map(|&w| {
            let plan = Plan {
                corpus,
                start_year,
                first_year_fraction,
                return_pct,
                inflation_pct,
                withdrawal: w * per_year,
                extras: extras.iter().map(|(_, e)| e.clone()).collect(),
                legs: legs.clone(),
                shifts: Shifts::default(),
            };
            scenario(&plan, w, foreign, include_rows)
        })
        .collect();

    Ok(RetirementPlan {
        as_of: nw.as_of,
        assumptions: Assumptions {
            corpus: round_money(corpus),
            corpus_source: corpus_source.to_string(),
            start_year,
            first_year_fraction: round_to(first_year_fraction, 4),
            years: model::YEARS,
            return_pct,
            inflation_pct: round_to(inflation_pct, 2),
            inflation_source: inflation_source.to_string(),
            inflation_country,
            monthly,
            fx_trend,
            inr_other_pct: round_to(inr_other, 2),
            reference_from: model::REF_FIRST_YEAR,
            reference_to: model::REF_LAST_YEAR,
            legs: legs.iter().map(|l| leg_view(l, &base)).collect(),
            extras: extra_views(extras),
        },
        base_currency: base,
        scenarios,
        warnings,
    })
}

/// The currency assumptions a plan takes, as on the screen's Currency risk card.
pub(crate) struct FxInput {
    pub fx_trend: bool,
    pub inr_moves: Option<BTreeMap<String, f64>>,
    pub inr_other_pct: Option<f64>,
    pub fx_overrides: Option<BTreeMap<String, FxOverride>>,
}

/// The corpus by currency against `base`, each with its assumed drift and
/// volatility, and the rupee's yearly change used against currencies with no
/// history.
pub(crate) fn currency_legs(
    exposure: &[networth::CurrencyExposure],
    base: &str,
    fx: &FxInput,
    warnings: &mut Vec<String>,
) -> (Vec<Leg>, f64) {
    let fx_trend = fx.fx_trend;
    let base = base.to_string();
    let overrides: BTreeMap<String, FxOverride> =
        fx.fx_overrides.clone().unwrap_or_default().into_iter().map(|(k, v)| (k.to_uppercase(), v)).collect();
    let inr_moves: BTreeMap<String, f64> =
        fx.inr_moves.clone().unwrap_or_default().into_iter().map(|(k, v)| (k.to_uppercase(), v)).collect();

    // The rupee against each currency, as on the screen: given, else its
    // ten-year trend (or level), else the one figure for currencies with no
    // history. Each part's move against the base follows from the rupee's
    // against both.
    let (inr_other_trend, inr_other_volatility) = model::inr_against_others();
    let inr_other = fx.inr_other_pct.unwrap_or(if fx_trend { inr_other_trend } else { 0.0 });
    let inr_move = |c: &str| match inr_moves.get(c) {
        Some(&m) => m,
        None => model::fx_history("INR", c).map_or(inr_other, |h| if fx_trend { h.trend_pct } else { 0.0 }),
    };
    let exposures: Vec<_> = exposure.iter().filter(|c| c.value_base > 0.0).collect();
    let exposed: f64 = exposures.iter().map(|c| c.value_base).sum();
    let legs: Vec<Leg> = if exposed > 0.0 {
        exposures
            .iter()
            .map(|c| {
                let is_base = c.currency == base;
                let h = model::fx_history(&c.currency, &base);
                let o = overrides.get(&c.currency);
                let given = |code: &str| code == "INR" || inr_moves.contains_key(code);
                if !is_base && h.is_none() && o.is_none() && !(given(&c.currency) && given(&base)) {
                    warnings.push(format!(
                        "no exchange-rate history for {} against {base}: assumed the rupee moves {:.1}% a year against a currency with none",
                        c.currency,
                        inr_other
                    ));
                }
                let history_volatility = h.as_ref().map_or(inr_other_volatility, |h| h.volatility_pct);
                let drift = model::pair_drift(&c.currency, &base, &inr_move);
                Leg {
                    currency: c.currency.clone(),
                    weight: c.value_base / exposed,
                    drift_pct: if is_base { 0.0 } else { o.and_then(|o| o.drift_pct).unwrap_or(drift) },
                    volatility_pct: if is_base {
                        0.0
                    } else {
                        o.and_then(|o| o.volatility_pct).unwrap_or(history_volatility)
                    },
                }
            })
            .collect()
    } else {
        vec![Leg { currency: base.clone(), weight: 1.0, drift_pct: 0.0, volatility_pct: 0.0 }]
    };
    for code in overrides.keys().filter(|code| !legs.iter().any(|l| &l.currency == *code)) {
        warnings.push(format!("fx_overrides: none of the corpus is in {code}, so it was not used"));
    }
    for code in inr_moves.keys().filter(|code| **code != base && !legs.iter().any(|l| &l.currency == *code)) {
        warnings.push(format!("inr_moves: none of the corpus is in {code} and it is not the base, so it was not used"));
    }
    (legs, inr_other)
}

/// The additional amounts as the model takes them, each with its label.
pub(crate) fn extras_of(input: Option<Vec<ExtraInput>>) -> Vec<(Option<String>, Extra)> {
    input
        .unwrap_or_default()
        .into_iter()
        .map(|e| {
            let extra = Extra {
                amount: e.amount,
                from_year: e.from_year,
                to_year: e.to_year.unwrap_or(e.from_year),
                indexed: e.indexed.unwrap_or(true),
            };
            (e.label, extra)
        })
        .collect()
}

pub(crate) fn extra_views(extras: Vec<(Option<String>, Extra)>) -> Vec<ExtraView> {
    extras
        .into_iter()
        .map(|(label, e)| ExtraView { label, amount: e.amount, from_year: e.from_year, to_year: e.to_year, indexed: e.indexed })
        .collect()
}

fn scenario(plan: &Plan, withdrawal: f64, foreign: bool, include_rows: bool) -> Scenario {
    let projection = model::project(plan, None);
    let last = projection.rows.last().expect("every plan runs at least one year");
    let swp = model::swp(plan);
    let currency_risk = foreign.then(|| {
        let band = model::risk_band(plan, model::RISK_PATHS);
        let rounded = |v: Vec<f64>| -> Vec<f64> { v.into_iter().map(round_money).collect() };
        CurrencyRisk {
            paths: model::RISK_PATHS,
            survival_pct: round_to(band.survival * 100.0, 1),
            p10: rounded(band.p10),
            p50: rounded(band.p50),
            p90: rounded(band.p90),
        }
    });

    Scenario {
        withdrawal: round_money(withdrawal),
        withdrawal_yearly: round_money(plan.withdrawal),
        withdrawal_rate_pct: (plan.corpus > 0.0).then(|| round_to(plan.withdrawal / plan.corpus * 100.0, 2)),
        runs_out_in: projection.runs_out_in,
        years_lasted: projection.years_lasted,
        final_closing: round_money(last.closing),
        final_closing_real: round_money(last.closing_real),
        monthly_draw: MonthlyDraw {
            months_lasted: swp.months_lasted,
            years_lasted: round_to(swp.months_lasted.unwrap_or(model::YEARS * 12) as f64 / 12.0, 2),
            closing: swp.months_lasted.is_none().then(|| round_money(swp.closing)),
        },
        currency_risk,
        rows: if include_rows { projection.rows.iter().map(row_view).collect() } else { Vec::new() },
    }
}

fn row_view(r: &YearRow) -> RowView {
    RowView {
        year: r.year,
        index: r.index,
        opening: round_money(r.opening),
        withdrawal: round_money(r.withdrawal),
        extras: round_money(r.extras),
        returns: round_money(r.returns),
        fx_effect: round_money(r.fx_effect),
        closing: round_money(r.closing),
        closing_real: round_money(r.closing_real),
        status: match r.status {
            RowStatus::Ok => "ok",
            RowStatus::Low => "low",
            RowStatus::Negative => "negative",
        }
        .to_string(),
    }
}

pub(crate) fn leg_view(leg: &Leg, base: &str) -> LegView {
    LegView {
        currency: leg.currency.clone(),
        weight_pct: round_to(leg.weight * 100.0, 2),
        drift_pct: round_to(leg.drift_pct, 2),
        volatility_pct: round_to(leg.volatility_pct, 2),
        history: (leg.currency != base).then(|| model::fx_history(&leg.currency, base)).flatten().map(|h| FxHistoryView {
            total_change_pct: round_to(h.total_change_pct, 2),
            trend_pct: round_to(h.trend_pct, 2),
            volatility_pct: round_to(h.volatility_pct, 2),
        }),
    }
}

/// Two significant figures, as the screen's defaults: a default amount
/// should look typed, not computed.
pub(crate) fn roundish(v: f64) -> f64 {
    if v <= 0.0 {
        return 0.0;
    }
    let mag = 10f64.powf(v.log10().floor() - 1.0);
    round_money((v / mag).round() * mag)
}

pub(crate) fn within(name: &str, v: f64, lo: f64, hi: f64) -> AppResult<()> {
    if v.is_finite() && v >= lo && v <= hi {
        Ok(())
    } else {
        Err(AppError::bad(format!("{name} must be between {lo} and {hi}")))
    }
}

pub(crate) fn year(name: &str, y: i32) -> AppResult<()> {
    if (1900..=2300).contains(&y) {
        Ok(())
    } else {
        Err(AppError::bad(format!("{name} must be a calendar year")))
    }
}

fn validate(input: &RetirementInput) -> AppResult<()> {
    if let Some(c) = input.corpus {
        within("corpus", c, 0.0, MAX_AMOUNT)?;
    }
    if let Some(y) = input.start_year {
        year("start_year", y)?;
    }
    if let Some(r) = input.return_pct {
        within("return_pct", r, -50.0, 50.0)?;
    }
    if let Some(i) = input.inflation_pct {
        within("inflation_pct", i, -20.0, 50.0)?;
    }
    if let Some(ws) = &input.withdrawals {
        if ws.is_empty() || ws.len() > MAX_SCENARIOS {
            return Err(AppError::bad(format!("withdrawals takes 1 to {MAX_SCENARIOS} amounts")));
        }
        for w in ws {
            within("each withdrawal", *w, 0.0, MAX_AMOUNT)?;
        }
    }
    validate_extras(input.extras.as_deref())?;
    validate_fx(input.inr_moves.as_ref(), input.inr_other_pct, input.fx_overrides.as_ref())
}

pub(crate) fn validate_extras(extras: Option<&[ExtraInput]>) -> AppResult<()> {
    if let Some(extras) = extras {
        if extras.len() > MAX_EXTRAS {
            return Err(AppError::bad(format!("extras takes at most {MAX_EXTRAS} amounts")));
        }
        for e in extras {
            within("extras amount", e.amount, -MAX_AMOUNT, MAX_AMOUNT)?;
            year("extras from_year", e.from_year)?;
            if let Some(to) = e.to_year {
                year("extras to_year", to)?;
                if to < e.from_year {
                    return Err(AppError::bad("extras to_year must not be before from_year"));
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_fx(
    inr_moves: Option<&BTreeMap<String, f64>>,
    inr_other_pct: Option<f64>,
    fx_overrides: Option<&BTreeMap<String, FxOverride>>,
) -> AppResult<()> {
    for (code, m) in inr_moves.into_iter().flatten() {
        if code.len() != 3 || !code.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(AppError::bad("inr_moves is keyed by 3-letter ISO currency code"));
        }
        if code.eq_ignore_ascii_case("INR") {
            return Err(AppError::bad("inr_moves: the rupee does not move against itself"));
        }
        within("inr_moves", *m, -50.0, 50.0)?;
    }
    if let Some(m) = inr_other_pct {
        within("inr_other_pct", m, -50.0, 50.0)?;
    }
    for (code, o) in fx_overrides.into_iter().flatten() {
        if code.len() != 3 || !code.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(AppError::bad("fx_overrides is keyed by 3-letter ISO currency code"));
        }
        if let Some(d) = o.drift_pct {
            within("fx_overrides drift_pct", d, -50.0, 50.0)?;
        }
        if let Some(v) = o.volatility_pct {
            within("fx_overrides volatility_pct", v, 0.0, 100.0)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_look_typed() {
        assert_eq!(roundish(1_234_567.0), 1_200_000.0);
        assert_eq!(roundish(45_678.0), 46_000.0);
        assert_eq!(roundish(0.0), 0.0);
    }

    #[test]
    fn out_of_range_assumptions_are_refused() {
        let bad = |input: RetirementInput| assert!(validate(&input).is_err());
        bad(RetirementInput { corpus: Some(-1.0), ..Default::default() });
        bad(RetirementInput { return_pct: Some(f64::NAN), ..Default::default() });
        bad(RetirementInput { withdrawals: Some(vec![]), ..Default::default() });
        bad(RetirementInput { withdrawals: Some(vec![1.0; MAX_SCENARIOS + 1]), ..Default::default() });
        bad(RetirementInput {
            extras: Some(vec![ExtraInput { label: None, amount: 1.0, from_year: 2030, to_year: Some(2029), indexed: None }]),
            ..Default::default()
        });
        bad(RetirementInput {
            fx_overrides: Some(BTreeMap::from([("DOLLAR".to_string(), FxOverride { drift_pct: None, volatility_pct: None })])),
            ..Default::default()
        });
        bad(RetirementInput { inr_moves: Some(BTreeMap::from([("INR".to_string(), 1.0)])), ..Default::default() });
        bad(RetirementInput { inr_moves: Some(BTreeMap::from([("SGD".to_string(), 80.0)])), ..Default::default() });
        bad(RetirementInput { inr_other_pct: Some(f64::NAN), ..Default::default() });
        assert!(validate(&RetirementInput::default()).is_ok());
    }
}
