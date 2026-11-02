//! CaiYatra - the journey of money: the CaiYatra screen's simulation on
//! the server, so the API - and the MCP tool drawn from it - give the
//! screen's simulation table.
//!
//! The corpus is built up from today until the retirement year and drawn
//! down from it, once per withdrawal (Sim 1, Sim 2, ...). The arithmetic is
//! [`crate::domain::retirement::project_flow`]; this fills in what the
//! caller leaves out the way the screen does and says in the answer what it
//! used. Any year, or run of years, can have its return, inflation or
//! withdrawal moved off the plan, as the table's rope does.
//!
//! The answer holds the table's figures and nothing else per row: the rates
//! each year runs at are inputs, given back once under `assumptions`.

use std::collections::BTreeMap;

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::domain::money::{round_money, round_to};
use crate::domain::retirement::{self as model, FlowPlan, Plan, PreWithdrawal, RowStatus, Shifts, YearRow};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

use super::networth;
use super::retirement::{
    currency_legs, extra_views, extras_of, leg_view, roundish, validate_extras, validate_fx, within, year,
    CurrencyRisk, ExtraInput, ExtraView, FxInput, FxOverride, LegView, DEFAULT_RETURN, FALLBACK_INFLATION,
    MAX_AMOUNT,
};

/// Withdrawals from retirement, as a share of the corpus at retirement each year.
const DEFAULT_RATES: [f64; 4] = [3.0, 4.0, 5.0, 6.0];
/// Retirement, when nothing is given: at this age, else this many years out.
const DEFAULT_RETIRE_AGE: u32 = 60;
const DEFAULT_YEARS_TO_RETIRE: i32 = 15;
/// How far a journey reaches, as the screen allows: retirement at most this
/// many years out and at most this age; an age today under 100.
const MAX_YEARS_TO_RETIRE: i32 = 50;
const MAX_RETIRE_AGE: u32 = 80;
const MAX_AGE: u32 = 99;
/// A share of the corpus a year, when withdrawals before retirement are a share.
const DEFAULT_PRE_PCT: f64 = 2.0;
/// A share of today's corpus, when withdrawals before retirement are an amount.
const DEFAULT_PRE_AMOUNT_PCT: f64 = 3.0;
const MAX_SIMS: usize = 10;
const MAX_ADJUSTMENTS: usize = 200;

/// The journey to run. Every field is optional: what is left out follows
/// the portfolio, history or the screen's defaults.
#[derive(Clone, Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DhanYatraInput {
    /// Currency every amount is counted in, given and answered. Omit for
    /// the portfolio's base currency.
    pub currency: Option<String>,
    /// Corpus today, in `currency`. Omit for today's net worth.
    pub corpus: Option<f64>,
    /// Age today, under 100. Optional: it adds an age to each row and allows `retire_age`.
    pub age: Option<u32>,
    /// Calendar year withdrawals begin - the tipping point. Omit for the
    /// year of `retire_age`, else age 60 when `age` is given, else 15 years
    /// from now. Not before this year, at most 50 years from now, and with
    /// `age`, at most the year they turn 80.
    pub retire_year: Option<i32>,
    /// Age at retirement, at most 80, instead of `retire_year`; needs `age`.
    pub retire_age: Option<u32>,
    /// Assumed yearly return, %, earned by each currency's part in its own
    /// currency. Default 8.
    pub return_pct: Option<f64>,
    /// Assumed yearly inflation, %. Omit for the ten-year average of
    /// `currency`'s country (5 where there is no series for it).
    pub inflation_pct: Option<f64>,
    /// What comes out each year before retirement: `none` (default - the
    /// corpus only grows), `pct` (a share of each year's opening balance) or
    /// `amount` (in today's money, rising with inflation).
    pub pre_retirement: Option<PreKind>,
    /// For `pct`: the share, %. Default 2.
    pub pre_retirement_pct: Option<f64>,
    /// For `amount`: per month when `monthly`, otherwise per year, today's
    /// money. Default 3% of the corpus today a year.
    pub pre_retirement_amount: Option<f64>,
    /// The withdrawals from retirement to compare, one Sim each, in today's
    /// money - per month when `monthly`, otherwise per year. Each rises
    /// with inflation. Omit for 3, 4, 5 and 6% of the corpus at retirement
    /// (in today's money) a year. At most 10.
    pub withdrawals: Option<Vec<f64>>,
    /// `withdrawals` and `pre_retirement_amount` are monthly. Default false.
    pub monthly: Option<bool>,
    /// Funds added or removed: savings until retirement, a bonus, a pension
    /// (positive); a wedding, a car (negative). At most 50.
    pub extras: Option<Vec<ExtraInput>>,
    /// Years moved off the plan - the screen's rope. Each moves one
    /// assumption by `points` for a year or a run of years; two on the same
    /// year and assumption add up. At most 200.
    pub adjustments: Option<Vec<Adjustment>>,
    /// Apply the rupee's ten-year trend against each currency; false holds
    /// every currency level. Default false.
    pub fx_trend: Option<bool>,
    /// By currency code: the rupee's assumed yearly change against it, %.
    /// -4 means a rupee buys 4% less of it each year.
    pub inr_moves: Option<BTreeMap<String, f64>>,
    /// The rupee's yearly change, %, against currencies with no history.
    pub inr_other_pct: Option<f64>,
    /// By currency code: replace the drift or volatility against `currency`
    /// outright. A drift here wins over `inr_moves`.
    pub fx_overrides: Option<BTreeMap<String, FxOverride>>,
    /// Include the year-by-year table in each Sim. Default true.
    pub include_rows: Option<bool>,
    /// Run each Sim 500 times with currencies moving at random, where part
    /// of the corpus is in another currency. Default true.
    pub currency_risk: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum PreKind {
    None,
    Pct,
    Amount,
}

/// One assumption moved off the plan for a year or a run of years.
#[derive(Clone, Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Adjustment {
    pub assumption: AdjustKind,
    /// First calendar year it applies to.
    pub from_year: i32,
    /// Last calendar year, inclusive. Omit for `from_year` alone.
    pub to_year: Option<i32>,
    /// Percentage points on top of the plan for each of those years. For
    /// `return` and `inflation`, on the rate: -10 turns 8% into -2%. For
    /// `withdrawal`, a share of that year's opening balance drawn on top
    /// (or, negative, held back - never below nothing).
    pub points: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum AdjustKind {
    Return,
    Inflation,
    Withdrawal,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct DhanYatraPlan {
    /// What every amount is in: `currency` as given, else the portfolio's base.
    pub currency: String,
    /// The date of the portfolio figures used.
    pub as_of: NaiveDate,
    pub assumptions: JourneyAssumptions,
    /// The corpus as retirement begins: the same for every Sim.
    pub retirement: AtRetirement,
    /// One per withdrawal, in the order given.
    pub sims: Vec<Sim>,
    pub warnings: Vec<String>,
}

/// What was assumed - the caller's figures, and what filled the rest.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct JourneyAssumptions {
    pub corpus: f64,
    /// `portfolio` (today's net worth) or `given`.
    pub corpus_source: String,
    /// The journey starts today, in this year.
    pub start_year: i32,
    /// How much of `start_year` the first row covers, 0-1: what is left of
    /// it. That row's withdrawal, recurring extras and return are for that
    /// share only.
    pub first_year_fraction: f64,
    /// Rows in each Sim before the table stops: at least 50, and 40 past retirement.
    pub years: u32,
    pub age: Option<u32>,
    pub retire_year: i32,
    pub return_pct: f64,
    pub inflation_pct: f64,
    /// `given`, `history` (the currency's ten-year average) or `fallback`.
    pub inflation_source: String,
    pub inflation_country: Option<String>,
    pub pre_retirement: PreKind,
    /// For `pct`, the share of each year's opening balance, %.
    pub pre_retirement_pct: Option<f64>,
    /// For `amount`, a year in today's money.
    pub pre_retirement_yearly: Option<f64>,
    pub monthly: bool,
    /// The years moved off the plan, by assumption and calendar year: points
    /// on top of `return_pct`, `inflation_pct`, or of the withdrawal as a
    /// share of the opening balance.
    pub adjustments: AdjustedYears,
    pub fx_trend: bool,
    pub inr_other_pct: f64,
    pub reference_from: i32,
    pub reference_to: i32,
    /// The corpus by currency, held at these weights every year.
    pub legs: Vec<LegView>,
    pub extras: Vec<ExtraView>,
}

#[derive(Clone, Debug, Default, Serialize, utoipa::ToSchema)]
pub struct AdjustedYears {
    #[serde(rename = "return")]
    pub return_pct: BTreeMap<i32, f64>,
    pub inflation: BTreeMap<i32, f64>,
    pub withdrawal: BTreeMap<i32, f64>,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct AtRetirement {
    pub year: i32,
    pub age: Option<u32>,
    /// The row number (`year_number`) of the first retired year.
    pub year_number: u32,
    /// The balance as retirement begins, and the same in today's money.
    pub corpus: f64,
    pub corpus_real: f64,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct Sim {
    /// 1 for the first withdrawal.
    pub sim: u32,
    /// As given: per month when `monthly`, otherwise per year, today's money.
    pub withdrawal: f64,
    /// The same for a year, today's money.
    pub withdrawal_yearly: f64,
    /// The yearly withdrawal as a share of the corpus at retirement in
    /// today's money, %. Null when that is nothing.
    pub withdrawal_rate_pct: Option<f64>,
    /// The yearly withdrawal in the retirement year's money.
    pub withdrawal_at_retirement: f64,
    /// Calendar year the corpus first goes below zero; null when it lasts.
    pub runs_out_in: Option<i32>,
    pub runs_out_at_age: Option<u32>,
    /// The corpus is gone before retirement begins.
    pub fails_before_retiring: bool,
    /// Full years of retirement it covers; to the last row when it lasts.
    pub years_in_retirement: u32,
    /// The highest year-end balance and its year: the top of the journey.
    /// Today's corpus in `start_year` when it only falls.
    pub peak: Peak,
    /// The balance after the last year run - below zero when it ran out -
    /// and the same in today's money.
    pub final_closing: f64,
    pub final_closing_real: f64,
    /// The spread currency moves alone could give. Null when the corpus is
    /// all in `currency`, or `currency_risk` is false.
    pub currency_risk: Option<CurrencyRisk>,
    /// The simulation table, year by year. It stops at the first year that
    /// opens and closes below zero. Empty when `include_rows` is false.
    pub rows: Vec<JourneyRow>,
}

#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct Peak {
    pub year: i32,
    pub age: Option<u32>,
    pub value: f64,
}

/// One row of the simulation table.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct JourneyRow {
    /// 1 for the first year of the journey.
    pub year_number: u32,
    pub year: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age: Option<u32>,
    pub opening: f64,
    pub withdrawal: f64,
    /// Funds added (positive) or removed (negative) that year.
    pub additional: f64,
    /// What the year's return earned.
    pub returns: f64,
    /// What currency moves added (or took) on top of the return.
    pub currency_impact: f64,
    pub closing: f64,
    /// The closing balance in today's money.
    pub closing_real: f64,
    /// `ok`, `low` (under two years of withdrawals left) or `negative`.
    pub status: String,
    /// The first year of retirement.
    pub retires: bool,
}

/// What a journey starts from: the owner's portfolio, or - for a visitor who
/// has not signed in - the guest page's example.
pub struct Start {
    pub base_currency: String,
    pub as_of: NaiveDate,
    pub total_base: f64,
    pub total_values: BTreeMap<String, Option<f64>>,
    pub currency_exposure: Vec<networth::CurrencyExposure>,
    pub warnings: Vec<String>,
}

impl From<networth::NetWorth> for Start {
    fn from(nw: networth::NetWorth) -> Self {
        Start {
            base_currency: nw.base_currency,
            as_of: nw.as_of,
            total_base: nw.total_base,
            total_values: nw.total_values,
            currency_exposure: nw.currency_exposure,
            warnings: nw.warnings,
        }
    }
}

impl Start {
    /// The guest page's example: `corpus` rupees, all of it in rupees.
    pub fn guest(corpus: f64) -> Start {
        Start {
            base_currency: "INR".to_string(),
            as_of: chrono::Utc::now().date_naive(),
            total_base: corpus,
            total_values: BTreeMap::from([("INR".to_string(), Some(corpus))]),
            currency_exposure: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

/// Run the journey once per withdrawal against the caller's portfolio.
pub async fn simulate(state: &AppState, owner: i32, input: DhanYatraInput) -> AppResult<DhanYatraPlan> {
    validate(&input)?;
    let nw = Start::from(networth::build(state, owner).await?);
    journey(&nw, input)
}

/// The journey from `nw`. Reads nothing: arithmetic on what is given.
pub fn journey(nw: &Start, input: DhanYatraInput) -> AppResult<DhanYatraPlan> {
    validate(&input)?;
    let mut warnings = Vec::new();

    let currency = input.currency.as_deref().map(str::to_uppercase).unwrap_or_else(|| nw.base_currency.clone());
    let to_currency = units_per_base(&nw, &currency)
        .ok_or_else(|| AppError::bad(format!("no exchange rate for {currency}: pick another currency")))?;
    let (corpus, corpus_source) = match input.corpus {
        Some(c) => (c, "given"),
        None => {
            warnings.extend(nw.warnings.iter().cloned());
            ((nw.total_base * to_currency).round(), "portfolio")
        }
    };

    let today = chrono::Utc::now().date_naive();
    let start_year = today.year();
    let first_year_fraction = model::first_year_fraction(start_year, today);
    let age = input.age;
    let mut retire_year = match (input.retire_year, input.retire_age, age) {
        (Some(y), _, _) => y,
        (None, Some(r), Some(a)) => start_year + r as i32 - a as i32,
        (None, _, Some(a)) => start_year + DEFAULT_RETIRE_AGE.saturating_sub(a) as i32,
        (None, _, None) => start_year + DEFAULT_YEARS_TO_RETIRE,
    };
    if retire_year < start_year {
        warnings.push(format!("retirement is before {start_year}: the journey draws down from today"));
        retire_year = start_year;
    }
    let age_in = |y: i32| age.map(|a| (a as i32 + y - start_year).max(0) as u32);

    let return_pct = input.return_pct.unwrap_or(DEFAULT_RETURN);
    let (inflation_pct, inflation_source, inflation_country) = match (input.inflation_pct, model::historical_inflation(&currency)) {
        (Some(i), _) => (i, "given", None),
        (None, Some((country, avg))) => (avg, "history", Some(country.to_string())),
        (None, None) => {
            warnings.push(format!("no inflation history for {currency}: assumed {FALLBACK_INFLATION}%"));
            (FALLBACK_INFLATION, "fallback", None)
        }
    };
    let monthly = input.monthly.unwrap_or(false);
    let per_year = if monthly { 12.0 } else { 1.0 };

    let pre_kind = input.pre_retirement.unwrap_or(PreKind::None);
    let pre_pct = input.pre_retirement_pct.unwrap_or(DEFAULT_PRE_PCT);
    let pre_yearly = input
        .pre_retirement_amount
        .map(|a| a * per_year)
        .unwrap_or_else(|| roundish(corpus * DEFAULT_PRE_AMOUNT_PCT / 100.0));
    let pre = match pre_kind {
        PreKind::None => PreWithdrawal::None,
        PreKind::Pct => PreWithdrawal::Pct(pre_pct),
        PreKind::Amount => PreWithdrawal::Amount(pre_yearly),
    };
    if pre_kind != PreKind::Pct && input.pre_retirement_pct.is_some() {
        warnings.push("pre_retirement_pct is only used when pre_retirement is 'pct'".to_string());
    }
    if pre_kind != PreKind::Amount && input.pre_retirement_amount.is_some() {
        warnings.push("pre_retirement_amount is only used when pre_retirement is 'amount'".to_string());
    }

    let fx_trend = input.fx_trend.unwrap_or(false);
    let (legs, inr_other) = currency_legs(
        &nw.currency_exposure,
        &currency,
        &FxInput {
            fx_trend,
            inr_moves: input.inr_moves.clone(),
            inr_other_pct: input.inr_other_pct,
            fx_overrides: input.fx_overrides.clone(),
        },
        &mut warnings,
    );
    let extras = extras_of(input.extras.clone());

    let years = model::flow_years(start_year, retire_year);
    let last_year = start_year + years as i32 - 1;
    let (shifts, adjusted) = shifts_of(input.adjustments.as_deref().unwrap_or_default());
    if input
        .adjustments
        .iter()
        .flatten()
        .any(|a| a.to_year.unwrap_or(a.from_year) < start_year || a.from_year > last_year)
    {
        warnings.push(format!("adjustments: only {start_year} to {last_year} are run; years outside it do nothing"));
    }

    let journey = |withdrawal: f64| FlowPlan {
        plan: Plan {
            corpus,
            start_year,
            first_year_fraction,
            return_pct,
            inflation_pct,
            withdrawal,
            extras: extras.iter().map(|(_, e)| e.clone()).collect(),
            legs: legs.clone(),
            shifts: shifts.clone(),
        },
        retire_year,
        pre,
        years,
    };

    // Up to retirement every Sim is the same journey; it sets the default withdrawals.
    let build = model::project_flow(&journey(0.0), None);
    let at_retirement_real = build.at_retirement_real.max(0.0);
    let withdrawals = input.withdrawals.clone().unwrap_or_else(|| {
        DEFAULT_RATES.iter().map(|r| roundish(at_retirement_real * r / 100.0) / per_year).collect()
    });

    let include_rows = input.include_rows.unwrap_or(true);
    let foreign = input.currency_risk.unwrap_or(true) && legs.iter().any(|l| l.currency != currency && l.volatility_pct > 0.0);
    let sims = withdrawals
        .iter()
        .enumerate()
        .map(|(i, &w)| {
            let flow_plan = journey(w * per_year);
            let flow = model::project_flow(&flow_plan, None);
            let rows = &flow.projection.rows;
            let last = rows.last().expect("every journey runs at least one year");
            let currency_risk = foreign.then(|| {
                let band = model::flow_band(&flow_plan, model::RISK_PATHS);
                let rounded = |v: Vec<f64>| -> Vec<f64> { v.into_iter().map(round_money).collect() };
                CurrencyRisk {
                    paths: model::RISK_PATHS,
                    survival_pct: round_to(band.survival * 100.0, 1),
                    p10: rounded(band.p10),
                    p50: rounded(band.p50),
                    p90: rounded(band.p90),
                }
            });
            // Once a year opens and closes below zero the plan has failed; the table stops there.
            let shown = rows.iter().position(|r| r.opening < 0.0 && r.closing < 0.0).map_or(rows.len(), |end| end + 1);
            Sim {
                sim: i as u32 + 1,
                withdrawal: round_money(w),
                withdrawal_yearly: round_money(w * per_year),
                withdrawal_rate_pct: (at_retirement_real > 0.0)
                    .then(|| round_to(w * per_year / at_retirement_real * 100.0, 2)),
                withdrawal_at_retirement: round_money(w * per_year * flow_plan.plan.price_level(flow.retire_index)),
                runs_out_in: flow.projection.runs_out_in,
                runs_out_at_age: flow.projection.runs_out_in.and_then(age_in),
                fails_before_retiring: flow.fails_before_retiring,
                years_in_retirement: flow.years_in_retirement,
                peak: Peak { year: flow.peak_year, age: age_in(flow.peak_year), value: round_money(flow.peak_value) },
                final_closing: round_money(last.closing),
                final_closing_real: round_money(last.closing_real),
                currency_risk,
                rows: if include_rows {
                    rows[..shown].iter().map(|r| row_view(r, age_in(r.year), r.year == retire_year)).collect()
                } else {
                    Vec::new()
                },
            }
        })
        .collect();

    Ok(DhanYatraPlan {
        as_of: nw.as_of,
        assumptions: JourneyAssumptions {
            corpus: round_money(corpus),
            corpus_source: corpus_source.to_string(),
            start_year,
            first_year_fraction: round_to(first_year_fraction, 4),
            years,
            age,
            retire_year,
            return_pct,
            inflation_pct: round_to(inflation_pct, 2),
            inflation_source: inflation_source.to_string(),
            inflation_country,
            pre_retirement: pre_kind,
            pre_retirement_pct: (pre_kind == PreKind::Pct).then_some(pre_pct),
            pre_retirement_yearly: (pre_kind == PreKind::Amount).then(|| round_money(pre_yearly)),
            monthly,
            adjustments: adjusted,
            fx_trend,
            inr_other_pct: round_to(inr_other, 2),
            reference_from: model::REF_FIRST_YEAR,
            reference_to: model::REF_LAST_YEAR,
            legs: legs.iter().map(|l| leg_view(l, &currency)).collect(),
            extras: extra_views(extras),
        },
        retirement: AtRetirement {
            year: retire_year,
            age: age_in(retire_year),
            year_number: build.retire_index,
            corpus: round_money(build.at_retirement),
            corpus_real: round_money(build.at_retirement_real),
        },
        currency,
        sims,
        warnings,
    })
}

/// Units of `currency` to one unit of the portfolio's base, as the screen
/// converts: the portfolio's own totals, else its exchange rate for the
/// currency, else the reference rates.
fn units_per_base(nw: &Start, currency: &str) -> Option<f64> {
    if currency == nw.base_currency {
        return Some(1.0);
    }
    if let Some(Some(total)) = nw.total_values.get(currency) {
        if *total > 0.0 && nw.total_base > 0.0 {
            return Some(total / nw.total_base);
        }
    }
    if let Some(e) = nw.currency_exposure.iter().find(|e| e.currency == currency && e.fx_rate > 0.0) {
        return Some(1.0 / e.fx_rate);
    }
    Some(model::inr_per_unit(&nw.base_currency)? / model::inr_per_unit(currency)?)
}

/// The adjustments by assumption and calendar year, ranges spread out and
/// overlaps added up.
fn shifts_of(adjustments: &[Adjustment]) -> (Shifts, AdjustedYears) {
    let mut shifts = Shifts::default();
    for a in adjustments {
        let years = match a.assumption {
            AdjustKind::Return => &mut shifts.return_pct,
            AdjustKind::Inflation => &mut shifts.inflation_pct,
            AdjustKind::Withdrawal => &mut shifts.withdrawal_pct,
        };
        for y in a.from_year..=a.to_year.unwrap_or(a.from_year) {
            *years.entry(y).or_insert(0.0) += a.points;
        }
    }
    for years in [&mut shifts.return_pct, &mut shifts.inflation_pct, &mut shifts.withdrawal_pct] {
        years.retain(|_, points| *points != 0.0);
    }
    let adjusted = AdjustedYears {
        return_pct: shifts.return_pct.clone(),
        inflation: shifts.inflation_pct.clone(),
        withdrawal: shifts.withdrawal_pct.clone(),
    };
    (shifts, adjusted)
}

fn row_view(r: &YearRow, age: Option<u32>, retires: bool) -> JourneyRow {
    JourneyRow {
        year_number: r.index,
        year: r.year,
        age,
        opening: round_money(r.opening),
        withdrawal: round_money(r.withdrawal),
        additional: round_money(r.extras),
        returns: round_money(r.returns),
        currency_impact: round_money(r.fx_effect),
        closing: round_money(r.closing),
        closing_real: round_money(r.closing_real),
        status: match r.status {
            RowStatus::Ok => "ok",
            RowStatus::Low => "low",
            RowStatus::Negative => "negative",
        }
        .to_string(),
        retires,
    }
}

fn validate(input: &DhanYatraInput) -> AppResult<()> {
    if let Some(c) = &input.currency {
        if c.len() != 3 || !c.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(AppError::bad("currency is a 3-letter ISO currency code"));
        }
    }
    if let Some(c) = input.corpus {
        within("corpus", c, 0.0, MAX_AMOUNT)?;
    }
    if let Some(a) = input.age {
        within("age", a as f64, 0.0, MAX_AGE as f64)?;
    }
    let this_year = chrono::Utc::now().year();
    if let Some(y) = input.retire_year {
        year("retire_year", y)?;
        if y > this_year + MAX_YEARS_TO_RETIRE {
            return Err(AppError::bad(format!(
                "retire_year must be at most {MAX_YEARS_TO_RETIRE} years from now, {}",
                this_year + MAX_YEARS_TO_RETIRE
            )));
        }
        if input.retire_age.is_some() {
            return Err(AppError::bad("give retire_year or retire_age, not both"));
        }
        if let Some(a) = input.age {
            if a as i32 + y - this_year > MAX_RETIRE_AGE as i32 {
                return Err(AppError::bad(format!(
                    "retire_year makes the retirement age over {MAX_RETIRE_AGE}: at most {}",
                    (this_year + MAX_RETIRE_AGE as i32 - a as i32).max(this_year)
                )));
            }
        }
    }
    if let Some(r) = input.retire_age {
        within("retire_age", r as f64, 0.0, MAX_RETIRE_AGE as f64)?;
        if input.age.is_none() {
            return Err(AppError::bad("retire_age needs age, today's age"));
        }
    }
    if let Some(r) = input.return_pct {
        within("return_pct", r, -50.0, 50.0)?;
    }
    if let Some(i) = input.inflation_pct {
        within("inflation_pct", i, -20.0, 50.0)?;
    }
    if let Some(p) = input.pre_retirement_pct {
        within("pre_retirement_pct", p, 0.0, 100.0)?;
    }
    if let Some(a) = input.pre_retirement_amount {
        within("pre_retirement_amount", a, 0.0, MAX_AMOUNT)?;
    }
    if let Some(ws) = &input.withdrawals {
        if ws.is_empty() || ws.len() > MAX_SIMS {
            return Err(AppError::bad(format!("withdrawals takes 1 to {MAX_SIMS} amounts")));
        }
        for w in ws {
            within("each withdrawal", *w, 0.0, MAX_AMOUNT)?;
        }
    }
    validate_extras(input.extras.as_deref())?;
    if let Some(adjustments) = &input.adjustments {
        validate_adjustments(adjustments)?;
    }
    validate_fx(input.inr_moves.as_ref(), input.inr_other_pct, input.fx_overrides.as_ref())
}

fn validate_adjustments(adjustments: &[Adjustment]) -> AppResult<()> {
    if adjustments.len() > MAX_ADJUSTMENTS {
        return Err(AppError::bad(format!("adjustments takes at most {MAX_ADJUSTMENTS}")));
    }
    for a in adjustments {
        within("adjustments points", a.points, -50.0, 50.0)?;
        year("adjustments from_year", a.from_year)?;
        if let Some(to) = a.to_year {
            year("adjustments to_year", to)?;
            if to < a.from_year {
                return Err(AppError::bad("adjustments to_year must not be before from_year"));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The rope on the person's page
// ---------------------------------------------------------------------------

/// A change to the rope on the person's CaiYatra page as it stands: every
/// other input on the page stays as it is.
#[derive(Clone, Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RopeChange {
    /// Straighten these ropes first - every year of the assumption back on
    /// the plan - as the page's "Straighten the rope" does.
    pub straighten: Option<Vec<AdjustKind>>,
    /// `set` (default): each year in `adjustments` sits at its `points`,
    /// whatever the page had there; 0 puts it back on the plan. `add`: the
    /// points go on top of what the page has.
    pub mode: Option<RopeMode>,
    /// The years to move, as simulate_dhan_yatra takes them; two on the same
    /// year and assumption add up. At most 200.
    pub adjustments: Option<Vec<Adjustment>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum RopeMode {
    #[default]
    Set,
    Add,
}

/// What a rope change does to the page, read back.
#[derive(Clone, Debug, Serialize, utoipa::ToSchema)]
pub struct RopeChanged {
    /// The ropes straightened first.
    pub straightened: Vec<AdjustKind>,
    pub mode: RopeMode,
    /// By assumption and calendar year, the points each year is set to
    /// (`set`) or moved by (`add`); a 0 under `set` is a year back on the plan.
    pub years: AdjustedYears,
    pub note: String,
}

/// Check a rope change and say what it does. The page's inputs are kept in
/// the browser, so nothing is run or stored here.
pub fn rope_change(change: &RopeChange) -> AppResult<RopeChanged> {
    let adjustments = change.adjustments.as_deref().unwrap_or_default();
    let mut straightened: Vec<AdjustKind> = Vec::new();
    for &kind in change.straighten.as_deref().unwrap_or_default() {
        if !straightened.contains(&kind) {
            straightened.push(kind);
        }
    }
    if adjustments.is_empty() && straightened.is_empty() {
        return Err(AppError::bad("give adjustments, straighten, or both"));
    }
    validate_adjustments(adjustments)?;
    let mut years = AdjustedYears::default();
    for a in adjustments {
        let into = match a.assumption {
            AdjustKind::Return => &mut years.return_pct,
            AdjustKind::Inflation => &mut years.inflation,
            AdjustKind::Withdrawal => &mut years.withdrawal,
        };
        for y in a.from_year..=a.to_year.unwrap_or(a.from_year) {
            *into.entry(y).or_insert(0.0) += a.points;
        }
    }
    Ok(RopeChanged {
        straightened,
        mode: change.mode.unwrap_or_default(),
        years,
        note: "The rope on the person's CaiYatra page changes with this answer; the rest of the page stays as it \
               is. They can go back with Undo in the chat."
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adjustments_spread_over_their_years_and_add_up() {
        let (shifts, adjusted) = shifts_of(&[
            Adjustment { assumption: AdjustKind::Return, from_year: 2030, to_year: Some(2032), points: -10.0 },
            Adjustment { assumption: AdjustKind::Return, from_year: 2031, to_year: None, points: 4.0 },
            Adjustment { assumption: AdjustKind::Withdrawal, from_year: 2040, to_year: None, points: 5.0 },
            Adjustment { assumption: AdjustKind::Inflation, from_year: 2035, to_year: None, points: 0.0 },
        ]);
        assert_eq!(shifts.return_pct, BTreeMap::from([(2030, -10.0), (2031, -6.0), (2032, -10.0)]));
        assert_eq!(shifts.withdrawal_pct, BTreeMap::from([(2040, 5.0)]));
        assert!(shifts.inflation_pct.is_empty(), "a zero shift is back on the plan");
        assert_eq!(adjusted.return_pct.len(), 3);
    }

    #[test]
    fn out_of_range_journeys_are_refused() {
        let bad = |input: DhanYatraInput| assert!(validate(&input).is_err());
        bad(DhanYatraInput { retire_age: Some(60), ..Default::default() });
        bad(DhanYatraInput { retire_year: Some(2040), retire_age: Some(60), age: Some(40), ..Default::default() });
        bad(DhanYatraInput { currency: Some("RUPEE".into()), ..Default::default() });
        bad(DhanYatraInput { pre_retirement_pct: Some(-1.0), ..Default::default() });
        bad(DhanYatraInput { withdrawals: Some(vec![]), ..Default::default() });
        let now = chrono::Utc::now().year();
        bad(DhanYatraInput { retire_year: Some(now * 10 + 7), ..Default::default() });
        bad(DhanYatraInput { retire_year: Some(now + 51), ..Default::default() });
        bad(DhanYatraInput { age: Some(100), ..Default::default() });
        bad(DhanYatraInput { age: Some(40), retire_age: Some(81), ..Default::default() });
        bad(DhanYatraInput { age: Some(40), retire_year: Some(now + 41), ..Default::default() });
        assert!(validate(&DhanYatraInput { age: Some(40), retire_year: Some(now + 40), ..Default::default() }).is_ok());
        assert!(validate(&DhanYatraInput { retire_year: Some(now + 50), ..Default::default() }).is_ok());
        bad(DhanYatraInput {
            adjustments: Some(vec![Adjustment {
                assumption: AdjustKind::Return,
                from_year: 2031,
                to_year: Some(2030),
                points: 1.0,
            }]),
            ..Default::default()
        });
        assert!(validate(&DhanYatraInput::default()).is_ok());
    }

    #[test]
    fn a_rope_change_reads_back_its_years_zeros_included() {
        let adj = |assumption, from_year, to_year, points| Adjustment { assumption, from_year, to_year, points };
        let changed = rope_change(&RopeChange {
            straighten: Some(vec![AdjustKind::Inflation]),
            mode: None,
            adjustments: Some(vec![adj(AdjustKind::Return, 2030, Some(2031), -10.0), adj(AdjustKind::Withdrawal, 2040, None, 0.0)]),
        })
        .unwrap();
        assert_eq!(changed.mode, RopeMode::Set);
        assert_eq!(changed.straightened, vec![AdjustKind::Inflation]);
        assert_eq!(changed.years.return_pct, BTreeMap::from([(2030, -10.0), (2031, -10.0)]));
        assert_eq!(changed.years.withdrawal, BTreeMap::from([(2040, 0.0)]), "set to 0: back on the plan");

        assert!(rope_change(&RopeChange::default()).is_err(), "nothing to change");
        assert!(rope_change(&RopeChange { adjustments: Some(vec![adj(AdjustKind::Return, 2030, None, 60.0)]), ..Default::default() }).is_err());
        assert!(rope_change(&RopeChange { straighten: Some(vec![AdjustKind::Return]), ..Default::default() }).is_ok());
    }
}
