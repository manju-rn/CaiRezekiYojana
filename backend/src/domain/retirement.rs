//! Retirement drawdown: how long a corpus lasts at a given withdrawal.
//!
//! The same arithmetic as the Retirement screen (`frontend/src/lib/retirement.ts`),
//! so the API - and the MCP tools drawn from it - give the screen's answer.
//! The reference series below are the screen's too; change them in both.
//!
//! [`project`] is the screen's model: one step per year, the withdrawal and
//! any additional amounts at the start of the year, then a year's return on
//! what is left. A plan that starts this year starts today, so its first
//! year is only what is left of the calendar year: that share of the
//! withdrawal, of recurring additional amounts and of the year's growth.
//! Later years are whole calendar years, with inflation counted from today.
//! The mix across currencies is held at today's weights. A
//! corpus that has run out earns nothing and keeps accruing the shortfall.
//!
//! [`swp`] is a systematic withdrawal plan on the same assumptions, drawn
//! month by month: the year's withdrawal in twelve parts, each month's
//! return on what is left. Money stays invested longer, so it lasts a little
//! longer than the yearly model says.
//!
//! [`project_flow`] is CaiYatra (`frontend/src/lib/dhanYatra.ts`): the
//! same yearly step over a whole journey, built up from today and drawn down
//! from a retirement year, with any year's return, inflation or withdrawal
//! shifted on its own ([`Shifts`]).

use std::collections::BTreeMap;

// -- Reference data -----------------------------------------------------------

/// Calendar years the reference series cover: ten annual changes.
pub const REF_FIRST_YEAR: i32 = 2015;
pub const REF_LAST_YEAR: i32 = 2025;

/// Consumer-price inflation, % change in the annual average, 2016-2025.
/// World Bank FP.CPI.TOTL.ZG (India, Singapore, Australia, US), Eurostat HICP
/// (euro area), ONS CPI (UK). The 2025 figures are provisional.
const INFLATION: &[(&str, &str, [f64; 10])] = &[
    ("INR", "India", [4.95, 3.33, 3.94, 3.73, 6.62, 5.13, 6.7, 5.65, 4.95, 2.2]),
    ("SGD", "Singapore", [-0.53, 0.58, 0.44, 0.57, -0.18, 2.3, 6.12, 4.82, 2.39, 0.9]),
    ("AUD", "Australia", [1.28, 1.95, 1.91, 1.61, 0.85, 2.86, 6.59, 5.6, 3.16, 2.8]),
    ("USD", "United States", [1.26, 2.13, 2.44, 1.81, 1.23, 4.7, 8.0, 4.12, 2.95, 2.7]),
    ("GBP", "United Kingdom", [0.7, 2.7, 2.5, 1.8, 0.9, 2.6, 9.1, 7.3, 2.5, 3.4]),
    ("EUR", "Euro area", [0.2, 1.5, 1.8, 1.2, 0.3, 2.6, 8.4, 5.4, 2.4, 2.1]),
];

/// Rupees per unit of each currency at each year end, 2015-2025 (reference
/// rates, rounded). Any pair is the ratio of two rows.
const INR_PER_UNIT: &[(&str, [f64; 11])] = &[
    ("INR", [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]),
    ("USD", [66.2, 67.9, 63.9, 69.8, 71.3, 73.1, 74.3, 82.7, 83.2, 85.6, 89.9]),
    ("SGD", [46.8, 47.0, 47.8, 51.2, 53.0, 55.3, 55.1, 61.7, 63.0, 62.8, 70.0]),
    ("AUD", [48.3, 49.0, 49.8, 49.2, 50.1, 56.3, 54.0, 56.3, 56.7, 53.0, 60.0]),
    ("GBP", [97.6, 83.8, 86.3, 89.0, 94.5, 99.9, 100.5, 100.1, 105.9, 107.2, 120.9]),
    ("EUR", [71.9, 71.4, 76.7, 79.9, 79.9, 89.3, 84.5, 88.5, 91.9, 88.6, 105.5]),
];

/// Geometric mean of yearly percentages, as a percentage.
pub fn average_rate(rates: &[f64]) -> f64 {
    if rates.is_empty() {
        return 0.0;
    }
    let growth: f64 = rates.iter().map(|r| 1.0 + r / 100.0).product();
    (growth.powf(1.0 / rates.len() as f64) - 1.0) * 100.0
}

/// Ten-year average inflation for a currency's country, with the country's
/// name, or None if there is no series for it.
pub fn historical_inflation(currency: &str) -> Option<(&'static str, f64)> {
    INFLATION.iter().find(|(c, _, _)| *c == currency).map(|(_, country, rates)| (*country, average_rate(rates)))
}

/// Rupees per unit of `currency` at the end of the reference years, or None
/// if there is no series for it.
pub fn inr_per_unit(currency: &str) -> Option<f64> {
    INR_PER_UNIT.iter().find(|(c, _)| *c == currency).map(|(_, r)| r[r.len() - 1])
}

/// How a currency moved against the base over the reference decade.
#[derive(Clone, Debug, PartialEq)]
pub struct FxHistory {
    /// The whole ten years, %: +20 means the currency bought 20% more base.
    pub total_change_pct: f64,
    /// Compound annual change, %.
    pub trend_pct: f64,
    /// Standard deviation of the yearly change, %.
    pub volatility_pct: f64,
}

/// What `currency` did against `base`, or None where either has no series.
pub fn fx_history(currency: &str, base: &str) -> Option<FxHistory> {
    let row = |code: &str| INR_PER_UNIT.iter().find(|(c, _)| *c == code).map(|(_, r)| r);
    let (c, b) = (row(currency)?, row(base)?);
    let rates: Vec<f64> = c.iter().zip(b).map(|(c, b)| c / b).collect();
    let logs: Vec<f64> = rates.windows(2).map(|w| (w[1] / w[0]).ln()).collect();
    let mean = logs.iter().sum::<f64>() / logs.len() as f64;
    let variance = logs.iter().map(|l| (l - mean).powi(2)).sum::<f64>() / (logs.len() - 1) as f64;
    Some(FxHistory {
        total_change_pct: (rates[rates.len() - 1] / rates[0] - 1.0) * 100.0,
        trend_pct: (mean.exp() - 1.0) * 100.0,
        volatility_pct: variance.sqrt() * 100.0,
    })
}

// -- The rupee against each currency -------------------------------------------
//
// Every pair follows from the rupee's move against each side, so a currency
// with no series takes one shared assumption: by default the rupee's average
// against the ones that have one.

/// The rupee's average yearly change and volatility, %, against the
/// currencies with a series.
pub fn inr_against_others() -> (f64, f64) {
    let known: Vec<FxHistory> =
        INR_PER_UNIT.iter().filter(|(c, _)| *c != "INR").filter_map(|(c, _)| fx_history("INR", c)).collect();
    let trends: Vec<f64> = known.iter().map(|h| h.trend_pct).collect();
    let volatility = known.iter().map(|h| h.volatility_pct).sum::<f64>() / known.len() as f64;
    (average_rate(&trends), volatility)
}

/// The yearly change of `currency` against `base`, %, given the rupee's
/// yearly change against each (`inr_move`, % - negative: the rupee loses).
pub fn pair_drift(currency: &str, base: &str, inr_move: impl Fn(&str) -> f64) -> f64 {
    let m = |c: &str| if c == "INR" { 0.0 } else { inr_move(c) / 100.0 };
    ((1.0 + m(base)) / (1.0 + m(currency)) - 1.0) * 100.0
}

// -- The simulation -----------------------------------------------------------

/// Years every plan is run for.
pub const YEARS: u32 = 50;

/// One currency's share of the corpus, and what is assumed of it.
#[derive(Clone, Debug)]
pub struct Leg {
    pub currency: String,
    /// Fraction of the corpus, 0-1.
    pub weight: f64,
    /// Assumed yearly change against the base, %. Zero for the base itself.
    pub drift_pct: f64,
    /// Yearly volatility against the base, %, for the risk band.
    pub volatility_pct: f64,
}

/// An amount on top of the withdrawal: income in, or a spend out.
#[derive(Clone, Debug)]
pub struct Extra {
    /// Positive comes in (a pension, a sale), negative goes out (a wedding).
    pub amount: f64,
    /// Calendar years, inclusive. A one-off has `from_year == to_year`.
    pub from_year: i32,
    pub to_year: i32,
    /// Grows with inflation, from today's money.
    pub indexed: bool,
}

/// Per calendar year, percentage points on top of the plan's assumption
/// for that year alone: a bad year early in retirement, a big spend.
#[derive(Clone, Debug, Default)]
pub struct Shifts {
    pub return_pct: BTreeMap<i32, f64>,
    pub inflation_pct: BTreeMap<i32, f64>,
    /// CaiYatra: a share of the year's opening balance drawn on top of
    /// (or held back from) the withdrawal.
    pub withdrawal_pct: BTreeMap<i32, f64>,
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub corpus: f64,
    pub start_year: i32,
    /// How much of the first calendar year the plan runs, 0-1: what is left
    /// of this year when it starts this year, else 1. See [`first_year_fraction`].
    pub first_year_fraction: f64,
    pub return_pct: f64,
    pub inflation_pct: f64,
    /// Each year's withdrawal, in today's money; it grows with inflation.
    pub withdrawal: f64,
    pub extras: Vec<Extra>,
    pub legs: Vec<Leg>,
    pub shifts: Shifts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowStatus {
    Ok,
    /// Fewer than two years of withdrawals left at the year end.
    Low,
    Negative,
}

#[derive(Clone, Debug)]
pub struct YearRow {
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
    pub status: RowStatus,
}

#[derive(Clone, Debug)]
pub struct Projection {
    pub rows: Vec<YearRow>,
    /// Calendar year in which the corpus first goes below zero, or None.
    pub runs_out_in: Option<i32>,
    /// Full years it covers before then; [`YEARS`] when it never runs out.
    pub years_lasted: u32,
}

/// Fewer years of withdrawals than this left at a year end reads as "low".
const LOW_YEARS: f64 = 2.0;

/// Share of `start_year` still ahead on `today`: the days from today to the
/// year end, today included. 1 for a plan that starts in a later year (or an
/// earlier one, which is a what-if from its first of January).
pub fn first_year_fraction(start_year: i32, today: chrono::NaiveDate) -> f64 {
    use chrono::Datelike;
    if start_year != today.year() {
        return 1.0;
    }
    let (Some(jan1), Some(next_jan1)) = (
        chrono::NaiveDate::from_ymd_opt(start_year, 1, 1),
        chrono::NaiveDate::from_ymd_opt(start_year + 1, 1, 1),
    ) else {
        return 1.0;
    };
    (next_jan1 - today).num_days() as f64 / (next_jan1 - jan1).num_days() as f64
}

impl Plan {
    /// Length of the plan's `index`th year, in years: short only for the first.
    pub fn year_length(&self, index: u32) -> f64 {
        if index == 1 { self.first_year_fraction } else { 1.0 }
    }

    /// Years from the plan's start (today) to the start of its `index`th year.
    pub fn years_before(&self, index: u32) -> f64 {
        if index == 1 { 0.0 } else { self.first_year_fraction + index as f64 - 2.0 }
    }

    /// Calendar year of the plan's `index`th year.
    pub fn year_of(&self, index: u32) -> i32 {
        self.start_year + index as i32 - 1
    }

    /// The return in the plan's `index`th year, %, with that year's shift.
    pub fn return_in(&self, index: u32) -> f64 {
        self.return_pct + self.shifts.return_pct.get(&self.year_of(index)).copied().unwrap_or(0.0)
    }

    /// Inflation in the plan's `index`th year, %, with that year's shift.
    pub fn inflation_in(&self, index: u32) -> f64 {
        self.inflation_pct + self.shifts.inflation_pct.get(&self.year_of(index)).copied().unwrap_or(0.0)
    }

    /// Prices at the start of the plan's `index`th year against today's.
    pub fn price_level(&self, index: u32) -> f64 {
        if self.shifts.inflation_pct.is_empty() {
            return (1.0 + self.inflation_pct / 100.0).powf(self.years_before(index));
        }
        (1..index).map(|k| (1.0 + self.inflation_in(k) / 100.0).powf(self.year_length(k))).product()
    }

    /// A recurring amount runs for the part of a short year; a one-off is paid whole.
    fn extras_in(&self, year: i32, index: u32) -> f64 {
        self.extras
            .iter()
            .filter(|e| year >= e.from_year && year <= e.to_year)
            .map(|e| {
                let indexed = if e.indexed { self.price_level(index) } else { 1.0 };
                let share = if e.from_year == e.to_year { 1.0 } else { self.year_length(index) };
                e.amount * indexed * share
            })
            .sum()
    }

    /// The full yearly withdrawal rate at the start of the plan's `index`th
    /// year, inflated from today's money. A short first year draws its share.
    pub fn withdrawal_in(&self, index: u32) -> f64 {
        self.withdrawal * self.price_level(index)
    }

    /// One year's growth factor on the whole corpus at the assumed drifts:
    /// each leg earns the return in its own currency, then converts.
    fn yearly_growth(&self) -> f64 {
        let r = self.return_pct / 100.0;
        self.legs.iter().map(|l| l.weight * (1.0 + r) * (1.0 + l.drift_pct / 100.0)).sum()
    }
}

/// The plan year by year, with each currency drifting at its assumed rate.
/// `fx_draws`, when given, replaces the drift with one sampled change per
/// year per leg - see [`risk_band`].
pub fn project(plan: &Plan, fx_draws: Option<&[Vec<f64>]>) -> Projection {
    project_with(plan, YEARS, |index, _| plan.withdrawal_in(index) * plan.year_length(index), fx_draws)
}

/// The year-by-year step behind [`project`], for `years` years, taking each
/// year's withdrawal from `withdrawal_in`: given the plan year and its
/// opening balance, what comes out that year, already cut to a short first
/// year.
pub fn project_with(
    plan: &Plan,
    years: u32,
    withdrawal_in: impl Fn(u32, f64) -> f64,
    fx_draws: Option<&[Vec<f64>]>,
) -> Projection {
    let mut rows = Vec::with_capacity(years as usize);
    let mut balance = plan.corpus;
    let mut runs_out_in = None;
    let mut years_lasted = years;

    for index in 1..=years {
        let year = plan.year_of(index);
        let length = plan.year_length(index);
        let r = plan.return_in(index) / 100.0;
        let opening = balance;
        let withdrawal = withdrawal_in(index, opening);
        let extras = plan.extras_in(year, index);
        let invested = opening - withdrawal + extras;

        let (mut returns, mut fx_effect) = (0.0, 0.0);
        if invested > 0.0 {
            returns = invested * ((1.0 + r).powf(length) - 1.0);
            let growth: f64 = plan
                .legs
                .iter()
                .enumerate()
                .map(|(i, leg)| {
                    let fx = fx_draws.map_or(leg.drift_pct / 100.0, |d| d[index as usize - 1][i]);
                    leg.weight * (((1.0 + r) * (1.0 + fx)).powf(length) - 1.0)
                })
                .sum();
            fx_effect = invested * growth - returns;
        }
        let closing = invested + returns + fx_effect;
        balance = closing;
        if runs_out_in.is_none() && closing < 0.0 {
            runs_out_in = Some(year);
            years_lasted = (plan.years_before(index) + 1e-9).floor() as u32;
        }

        let next_withdrawal = withdrawal_in(index + 1, closing);
        rows.push(YearRow {
            year,
            index,
            opening,
            withdrawal,
            extras,
            returns,
            fx_effect,
            closing,
            closing_real: closing / plan.price_level(index + 1),
            status: if closing < 0.0 {
                RowStatus::Negative
            } else if closing < next_withdrawal * LOW_YEARS {
                RowStatus::Low
            } else {
                RowStatus::Ok
            },
        });
    }

    Projection { rows, runs_out_in, years_lasted }
}

/// A monthly withdrawal plan's outcome - see [`swp`].
#[derive(Clone, Debug, PartialEq)]
pub struct SwpRun {
    /// Full monthly withdrawals paid before the corpus falls short; None
    /// when it pays all `YEARS * 12`.
    pub months_lasted: Option<u32>,
    /// What is left after the last month, when it lasts.
    pub closing: f64,
}

/// The plan drawn monthly: each month a twelfth of that year's withdrawal
/// comes out, the year's additional amounts go in or out with its first
/// month, and what is left earns a month's share of the year's growth. A
/// short first year has as many months as are left of it, at least one.
pub fn swp(plan: &Plan) -> SwpRun {
    let monthly_growth = plan.yearly_growth().max(0.0).powf(1.0 / 12.0);
    let mut balance = plan.corpus;
    let mut month = 0;

    for index in 1..=YEARS {
        let months = if index == 1 { ((plan.first_year_fraction * 12.0).round() as u32).clamp(1, 12) } else { 12 };
        balance += plan.extras_in(plan.start_year + index as i32 - 1, index);
        for _ in 0..months {
            balance -= plan.withdrawal_in(index) / 12.0;
            if balance < 0.0 {
                return SwpRun { months_lasted: Some(month), closing: balance };
            }
            balance *= monthly_growth;
            month += 1;
        }
    }
    SwpRun { months_lasted: None, closing: balance }
}

/// Small seeded generator (mulberry32, as on the screen), so the band is
/// the same on every call.
struct Mulberry32(u32);

impl Mulberry32 {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let a = self.0;
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        (t ^ (t >> 14)) as f64 / 4_294_967_296.0
    }

    fn normal(&mut self) -> f64 {
        let u = self.next().max(1e-12);
        (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * self.next()).cos()
    }
}

#[derive(Clone, Debug)]
pub struct RiskBand {
    /// Per year: the 10th, 50th and 90th percentile closing balance.
    pub p10: Vec<f64>,
    pub p50: Vec<f64>,
    pub p90: Vec<f64>,
    /// Share of paths, 0-1, still above zero at the end.
    pub survival: f64,
}

pub const RISK_PATHS: usize = 500;

/// Currency risk: the same plan run many times, each foreign leg moving by a
/// random yearly change with its assumed drift and volatility (log-normal,
/// legs independent). The return itself is held fixed, so the spread is the
/// currency risk alone.
pub fn risk_band(plan: &Plan, paths: usize) -> RiskBand {
    sample_band(&plan.legs, YEARS, |draws| project(plan, Some(draws)), paths)
}

/// [`risk_band`] for any `years`-long projection over these currency legs.
pub fn sample_band(legs: &[Leg], years: u32, run: impl Fn(&[Vec<f64>]) -> Projection, paths: usize) -> RiskBand {
    let mut rand = Mulberry32(20261005);
    let mut by_year: Vec<Vec<f64>> = vec![Vec::with_capacity(paths); years as usize];
    let mut survived = 0;

    for _ in 0..paths {
        let draws: Vec<Vec<f64>> = (0..years)
            .map(|_| {
                legs.iter()
                    .map(|leg| {
                        if leg.volatility_pct == 0.0 {
                            return leg.drift_pct / 100.0;
                        }
                        let sigma = leg.volatility_pct / 100.0;
                        // Mean of the log change chosen so the expected change is the drift.
                        let mu = (1.0 + leg.drift_pct / 100.0).ln() - sigma * sigma / 2.0;
                        (mu + sigma * rand.normal()).exp() - 1.0
                    })
                    .collect()
            })
            .collect();
        let result = run(&draws);
        for (i, row) in result.rows.iter().enumerate() {
            by_year[i].push(row.closing);
        }
        if result.runs_out_in.is_none() {
            survived += 1;
        }
    }

    for values in &mut by_year {
        values.sort_by(f64::total_cmp);
    }
    let at = |q: f64| -> Vec<f64> {
        by_year.iter().map(|v| v[((q * v.len() as f64).floor() as usize).min(v.len() - 1)]).collect()
    };
    RiskBand { p10: at(0.1), p50: at(0.5), p90: at(0.9), survival: survived as f64 / paths as f64 }
}

// -- CaiYatra: the whole journey --------------------------------------------

/// Retirement still needs this many years of drawdown.
pub const RETIRED_YEARS: u32 = 40;

/// What comes out before the retirement year.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PreWithdrawal {
    /// Nothing: the corpus grows on its return and the additional amounts.
    None,
    /// A share of each year's opening balance, %.
    Pct(f64),
    /// A year, in today's money; it grows with inflation.
    Amount(f64),
}

/// A plan over the whole journey. `plan.withdrawal` applies from
/// `retire_year`, in today's money and rising with inflation.
#[derive(Clone, Debug)]
pub struct FlowPlan {
    pub plan: Plan,
    /// Calendar year withdrawals at `plan.withdrawal` begin.
    pub retire_year: i32,
    pub pre: PreWithdrawal,
    /// How many years the plan runs, the first (short) one included.
    pub years: u32,
}

/// Years to run: at least [`YEARS`], and [`RETIRED_YEARS`] past retirement.
pub fn flow_years(start_year: i32, retire_year: i32) -> u32 {
    YEARS.max((retire_year - start_year).max(0) as u32 + RETIRED_YEARS)
}

impl FlowPlan {
    /// Plan year (1-based) in which retirement begins.
    pub fn retire_index(&self) -> u32 {
        (self.retire_year - self.plan.start_year + 1).max(1) as u32
    }

    /// What comes out in plan year `index` before that year's shift, given its opening balance.
    fn planned_withdrawal(&self, index: u32, opening: f64) -> f64 {
        let p = &self.plan;
        let length = p.year_length(index);
        if index >= self.retire_index() {
            return p.withdrawal * p.price_level(index) * length;
        }
        match self.pre {
            PreWithdrawal::None => 0.0,
            PreWithdrawal::Pct(pct) if opening > 0.0 => opening * pct / 100.0 * length,
            PreWithdrawal::Pct(_) => 0.0,
            PreWithdrawal::Amount(amount) => amount * p.price_level(index) * length,
        }
    }

    /// What comes out in plan year `index`, given its opening balance: a
    /// shifted year takes that share of it more or less, never below nothing.
    pub fn withdrawal(&self, index: u32, opening: f64) -> f64 {
        let planned = self.planned_withdrawal(index, opening);
        let shift = self.plan.shifts.withdrawal_pct.get(&self.plan.year_of(index)).copied().unwrap_or(0.0);
        if shift == 0.0 || opening <= 0.0 {
            return planned;
        }
        (planned + opening * shift / 100.0 * self.plan.year_length(index)).max(0.0)
    }
}

#[derive(Clone, Debug)]
pub struct Flow {
    pub projection: Projection,
    pub retire_index: u32,
    /// The balance as retirement begins, and the same in today's money.
    pub at_retirement: f64,
    pub at_retirement_real: f64,
    /// The highest year-end balance and its year; today's corpus in the
    /// start year when it only falls.
    pub peak_year: i32,
    pub peak_value: f64,
    /// Whether the corpus is gone before retirement begins.
    pub fails_before_retiring: bool,
    /// Full years of retirement it covers; to the horizon when it never runs out.
    pub years_in_retirement: u32,
}

/// The journey year by year; `fx_draws` as for [`project`].
pub fn project_flow(flow: &FlowPlan, fx_draws: Option<&[Vec<f64>]>) -> Flow {
    let plan = &flow.plan;
    let run = project_with(plan, flow.years, |index, opening| flow.withdrawal(index, opening), fx_draws);
    let retire_index = flow.retire_index();
    let at_retirement = if retire_index == 1 { plan.corpus } else { run.rows[retire_index as usize - 2].closing };
    let (peak_year, peak_value) = run
        .rows
        .iter()
        .fold((plan.start_year, plan.corpus), |best, r| if r.closing > best.1 { (r.year, r.closing) } else { best });
    let fail_index = run.runs_out_in.map(|y| (y - plan.start_year + 1) as u32);
    let years_in_retirement = match fail_index {
        None => flow.years - retire_index + 1,
        Some(f) => (plan.years_before(f) - plan.years_before(retire_index) + 1e-9).floor().max(0.0) as u32,
    };
    Flow {
        retire_index,
        at_retirement,
        at_retirement_real: at_retirement / plan.price_level(retire_index),
        peak_year,
        peak_value,
        fails_before_retiring: fail_index.is_some_and(|f| f < retire_index),
        years_in_retirement,
        projection: run,
    }
}

/// Currency risk over the journey: [`sample_band`] run over the whole flow.
pub fn flow_band(flow: &FlowPlan, paths: usize) -> RiskBand {
    sample_band(&flow.plan.legs, flow.years, |draws| project_flow(flow, Some(draws)).projection, paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(corpus: f64, return_pct: f64, inflation_pct: f64, withdrawal: f64) -> Plan {
        Plan {
            corpus,
            start_year: 2026,
            first_year_fraction: 1.0,
            return_pct,
            inflation_pct,
            withdrawal,
            extras: Vec::new(),
            legs: vec![Leg { currency: "INR".into(), weight: 1.0, drift_pct: 0.0, volatility_pct: 0.0 }],
            shifts: Shifts::default(),
        }
    }

    fn flow(p: Plan, retire_year: i32, pre: PreWithdrawal) -> FlowPlan {
        let years = flow_years(p.start_year, retire_year);
        FlowPlan { plan: p, retire_year, pre, years }
    }

    #[test]
    fn a_shifted_year_moves_only_that_year() {
        let mut p = plan(1_000_000.0, 10.0, 10.0, 0.0);
        p.shifts.return_pct.insert(2027, -20.0);
        p.shifts.inflation_pct.insert(2027, 10.0);
        let rows = project(&p, None).rows;
        assert!((rows[0].returns - 100_000.0).abs() < 1e-6);
        // 2027 earns 10 - 20 = -10%; 2028 is back to 10%.
        assert!((rows[1].returns + 110_000.0).abs() < 1e-6);
        assert!((rows[2].returns - 99_000.0).abs() < 1e-6);
        // Prices: 10% in 2026, 20% in 2027, 10% after.
        assert!((p.price_level(3) - 1.1 * 1.2).abs() < 1e-12);
        assert!((p.price_level(4) - 1.1 * 1.2 * 1.1).abs() < 1e-12);
    }

    #[test]
    fn the_journey_builds_up_then_draws_down() {
        // No return or inflation: 1M grows by 100k a year for five years, then 150k a year comes out.
        let mut p = plan(1_000_000.0, 0.0, 0.0, 150_000.0);
        p.extras.push(Extra { amount: 100_000.0, from_year: 2026, to_year: 2030, indexed: true });
        let f = project_flow(&flow(p, 2031, PreWithdrawal::None), None);
        assert_eq!(f.projection.rows.len(), 50);
        assert_eq!(f.retire_index, 6);
        assert_eq!(f.at_retirement, 1_500_000.0);
        assert_eq!((f.peak_year, f.peak_value), (2030, 1_500_000.0));
        assert_eq!(f.projection.rows[4].withdrawal, 0.0);
        assert_eq!(f.projection.rows[5].withdrawal, 150_000.0);
        // Ten years of 150k: 2031-2040 close at zero or above, 2041 below.
        assert_eq!(f.projection.runs_out_in, Some(2041));
        assert_eq!(f.years_in_retirement, 10);
        assert!(!f.fails_before_retiring);
    }

    #[test]
    fn withdrawals_before_retiring_by_share_or_amount() {
        let p = plan(1_000_000.0, 0.0, 10.0, 0.0);
        let pct = project_flow(&flow(p.clone(), 2030, PreWithdrawal::Pct(10.0)), None);
        assert_eq!(pct.projection.rows[0].withdrawal, 100_000.0);
        assert!((pct.projection.rows[1].withdrawal - 90_000.0).abs() < 1e-6);
        let amount = project_flow(&flow(p, 2030, PreWithdrawal::Amount(50_000.0)), None);
        assert!((amount.projection.rows[1].withdrawal - 55_000.0).abs() < 1e-6);
        assert!((amount.at_retirement_real - amount.at_retirement / 1.1_f64.powi(4)).abs() < 1e-6);
    }

    #[test]
    fn a_withdrawal_shift_takes_a_share_of_the_opening_balance() {
        let mut p = plan(1_000_000.0, 0.0, 0.0, 0.0);
        p.shifts.withdrawal_pct.insert(2027, 5.0);
        p.shifts.withdrawal_pct.insert(2028, -50.0);
        let f = project_flow(&flow(p, 2026, PreWithdrawal::None), None);
        assert_eq!(f.projection.rows[1].withdrawal, 50_000.0);
        // Held back below nothing is nothing.
        assert_eq!(f.projection.rows[2].withdrawal, 0.0);
    }

    #[test]
    fn running_out_before_retiring_is_said() {
        let p = plan(100_000.0, 0.0, 0.0, 0.0);
        let f = project_flow(&flow(p, 2040, PreWithdrawal::Amount(50_000.0)), None);
        assert_eq!(f.projection.runs_out_in, Some(2028));
        assert!(f.fails_before_retiring);
        assert_eq!(f.years_in_retirement, 0);
    }

    #[test]
    fn the_flow_band_covers_the_whole_journey() {
        let mut p = plan(10_000_000.0, 8.0, 5.0, 500_000.0);
        p.legs = vec![
            Leg { currency: "INR".into(), weight: 0.5, drift_pct: 0.0, volatility_pct: 0.0 },
            Leg { currency: "USD".into(), weight: 0.5, drift_pct: 3.0, volatility_pct: 4.0 },
        ];
        let f = flow(p, 2046, PreWithdrawal::None);
        assert_eq!(f.years, 60);
        assert_eq!(flow_band(&f, 50).p50.len(), 60);
    }

    #[test]
    fn ten_years_of_withdrawals_with_no_return_last_ten_years() {
        let p = project(&plan(1_000_000.0, 0.0, 0.0, 100_000.0), None);
        // Year 10 closes at exactly zero; year 11 is the first below it.
        assert_eq!(p.runs_out_in, Some(2036));
        assert_eq!(p.years_lasted, 10);
        assert_eq!(p.rows[9].closing, 0.0);
        assert_eq!(p.rows[10].status, RowStatus::Negative);

        let s = swp(&plan(1_000_000.0, 0.0, 0.0, 100_000.0));
        assert_eq!(s.months_lasted, Some(120));
    }

    #[test]
    fn a_withdrawal_below_the_real_return_lasts_the_horizon() {
        let p = project(&plan(10_000_000.0, 8.0, 5.0, 200_000.0), None);
        assert_eq!(p.runs_out_in, None);
        assert_eq!(p.years_lasted, YEARS);
        assert_eq!(swp(&plan(10_000_000.0, 8.0, 5.0, 200_000.0)).months_lasted, None);
    }

    #[test]
    fn monthly_draws_last_at_least_as_long_as_yearly_ones() {
        for rate in [4.0, 6.0, 8.0, 10.0] {
            let p = plan(50_000_000.0, 8.0, 6.0, 50_000_000.0 * rate / 100.0);
            let yearly = project(&p, None).years_lasted * 12;
            let monthly = swp(&p).months_lasted.unwrap_or(YEARS * 12);
            assert!(monthly >= yearly, "{rate}%: {monthly} months monthly, {yearly} yearly");
        }
    }

    #[test]
    fn extras_and_inflation_apply_by_calendar_year() {
        let mut p = plan(1_000_000.0, 0.0, 10.0, 100_000.0);
        p.extras.push(Extra { amount: 50_000.0, from_year: 2027, to_year: 2028, indexed: true });
        let rows = project(&p, None).rows;
        assert_eq!(rows[0].extras, 0.0);
        assert!((rows[1].extras - 55_000.0).abs() < 1e-6);
        assert!((rows[2].withdrawal - 121_000.0).abs() < 1e-6);
        assert_eq!(rows[3].extras, 0.0);
    }

    #[test]
    fn a_plan_started_this_year_runs_only_the_rest_of_it() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 7).unwrap();
        let f = first_year_fraction(2026, today);
        // 7 October to 31 December, both included: 86 of 365 days.
        assert!((f - 86.0 / 365.0).abs() < 1e-12, "{f}");
        assert_eq!(first_year_fraction(2027, today), 1.0);

        let mut p = plan(1_000_000.0, 10.0, 10.0, 100_000.0);
        p.first_year_fraction = f;
        p.extras.push(Extra { amount: 50_000.0, from_year: 2026, to_year: 2026, indexed: true });
        p.extras.push(Extra { amount: 20_000.0, from_year: 2026, to_year: 2030, indexed: false });
        let rows = project(&p, None).rows;
        assert!((rows[0].withdrawal - 100_000.0 * f).abs() < 1e-6);
        // The one-off is paid whole, the recurring amount for its share.
        assert!((rows[0].extras - (50_000.0 + 20_000.0 * f)).abs() < 1e-6);
        let invested = 1_000_000.0 - 100_000.0 * f + 50_000.0 + 20_000.0 * f;
        assert!((rows[0].returns - invested * (1.1_f64.powf(f) - 1.0)).abs() < 1e-6);
        // Year two is a whole year, its withdrawal inflated for the part year.
        assert!((rows[1].withdrawal - 100_000.0 * 1.1_f64.powf(f)).abs() < 1e-6);
        assert!((rows[1].closing_real - rows[1].closing / 1.1_f64.powf(1.0 + f)).abs() < 1e-6);
    }

    #[test]
    fn reference_averages_and_fx() {
        let (country, india) = historical_inflation("INR").unwrap();
        assert_eq!(country, "India");
        assert!((india - 4.71).abs() < 0.05, "{india}");
        assert!(historical_inflation("JPY").is_none());

        let usd = fx_history("USD", "INR").unwrap();
        assert!((usd.total_change_pct - (89.9 / 66.2 - 1.0) * 100.0).abs() < 1e-9);
        assert!(usd.trend_pct > 0.0 && usd.volatility_pct > 0.0);
        assert!(fx_history("INR", "INR").unwrap().volatility_pct.abs() < 1e-12);
        assert!(fx_history("JPY", "INR").is_none());
    }

    #[test]
    fn pairs_follow_the_rupee_against_each_side() {
        let inr_move = |c: &str| fx_history("INR", c).unwrap().trend_pct;
        for (c, b) in [("SGD", "INR"), ("INR", "SGD"), ("USD", "SGD"), ("EUR", "GBP")] {
            let direct = fx_history(c, b).unwrap().trend_pct;
            assert!((pair_drift(c, b, inr_move) - direct).abs() < 1e-9, "{c}/{b}");
        }
        // The rupee lost ground against the table on average, by about 3% a year.
        let (trend, volatility) = inr_against_others();
        assert!((trend + 3.0).abs() < 0.1, "{trend}");
        assert!(volatility > 0.0);
    }

    #[test]
    fn the_risk_band_is_ordered_and_repeatable() {
        let mut p = plan(50_000_000.0, 8.0, 5.0, 2_500_000.0);
        p.legs = vec![
            Leg { currency: "INR".into(), weight: 0.5, drift_pct: 0.0, volatility_pct: 0.0 },
            Leg { currency: "USD".into(), weight: 0.5, drift_pct: 3.0, volatility_pct: 4.0 },
        ];
        let a = risk_band(&p, 200);
        let b = risk_band(&p, 200);
        assert_eq!(a.p50, b.p50);
        assert!(a.p10.iter().zip(&a.p90).all(|(lo, hi)| lo <= hi));
        assert!((0.0..=1.0).contains(&a.survival));
    }

    #[test]
    fn the_generator_matches_the_screens() {
        // mulberry32(1) in JavaScript: 0.6270739405881613, 0.002735721180215478.
        let mut r = Mulberry32(1);
        assert!((r.next() - 0.627_073_940_588_161_3).abs() < 1e-15);
        assert!((r.next() - 0.002_735_721_180_215_478).abs() < 1e-15);
    }
}
