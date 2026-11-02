//! Term-deposit valuation: Indian FD/RD, Singapore fixed deposits, Australian
//! term deposits.
//!
//! Accrued value is computed on every read rather than stored, so a deposit
//! opened once is correct forever without a nightly job. Where a bank has
//! quoted a maturity figure we use theirs - banks round day counts and
//! compounding in house-specific ways and their number is the one that pays.

use std::str::FromStr;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::entities::deposit;
use crate::types::{Compounding, DepositKind, PayoutFrequency};

use super::money::round_money;

const DAYS_PER_YEAR: f64 = 365.0;

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DepositValuation {
    pub deposit_id: i32,
    pub currency: String,
    /// Cash actually put in so far. For an RD this grows with each instalment.
    pub principal_invested: f64,
    /// What it is worth today, including interest retained inside the deposit
    /// and net of any tax already withheld from it.
    pub current_value: f64,
    pub accrued_interest: f64,
    /// Interest already paid away to a bank account (non-cumulative deposits),
    /// after any tax withheld from it.
    pub interest_paid_out: f64,
    /// Gross: principal plus every rupee of interest, before tax withheld.
    /// The figure banks quote.
    pub maturity_value: f64,
    pub interest_at_maturity: f64,
    /// Tax withheld at source so far, out of the deposit or its payouts.
    pub tax_withheld: f64,
    /// The tax withheld over the whole term, what has already gone included.
    pub tax_at_maturity: f64,
    /// What actually comes back at maturity: `maturity_value` less the tax
    /// withheld along the way. The same as it when nothing is withheld.
    pub maturity_payout: f64,
    pub days_elapsed: i64,
    pub days_to_maturity: i64,
    pub term_days: i64,
    /// 0-100, how far through the term we are.
    pub progress_pct: f64,
    /// Annual yield the deposit actually delivers once compounding is counted.
    pub effective_annual_yield: f64,
    pub is_matured: bool,
    /// True when `maturity_value` came from the bank rather than our maths.
    pub maturity_is_bank_quoted: bool,
}

/// Grow `principal` for `years` at `rate_pct` per annum.
///
/// `SIMPLE` and `AT_MATURITY` both accrue simple interest: a sub-year term
/// deposit pays one lump of interest at the end, it does not compound.
pub fn grow(principal: f64, rate_pct: f64, compounding: Compounding, years: f64) -> f64 {
    if years <= 0.0 {
        return principal;
    }
    let r = rate_pct / 100.0;
    match compounding {
        Compounding::Simple | Compounding::AtMaturity => principal * (1.0 + r * years),
        _ => {
            let n = compounding.periods_per_year();
            principal * (1.0 + r / n).powf(n * years)
        }
    }
}

/// Recurring deposit: every instalment is a separate deposit that compounds
/// for however long it has left to run.
fn grow_recurring(
    installment: f64,
    rate_pct: f64,
    compounding: Compounding,
    start: NaiveDate,
    until: NaiveDate,
) -> (f64, f64) {
    let total_months = months_between(start, until);
    let mut value = 0.0;
    let mut paid = 0.0;
    for i in 0..total_months {
        let contributed_at = add_months(start, i);
        if contributed_at > until {
            break;
        }
        let years = (until - contributed_at).num_days() as f64 / DAYS_PER_YEAR;
        value += grow(installment, rate_pct, compounding, years);
        paid += installment;
    }
    (value, paid)
}

/// A cumulative deposit from `start` to `until`, with `tds` (0-1) of each
/// financial year's interest taken out of the deposit at 31 March, and of
/// the last stretch at maturity - the way an Indian bank withholds tax.
/// Interest then compounds on what is left. With `tds` at zero this is
/// exactly [`grow`]. Returns `(balance, tax withheld)`.
fn grow_withholding(
    principal: f64,
    rate_pct: f64,
    compounding: Compounding,
    start: NaiveDate,
    until: NaiveDate,
    maturity: NaiveDate,
    tds: f64,
) -> (f64, f64) {
    let simple = matches!(compounding, Compounding::Simple | Compounding::AtMaturity);
    let mut balance = principal;
    let mut withheld = 0.0;
    let mut from = start;
    while from < until {
        let year_end = financial_year_end(from);
        let to = year_end.min(until);
        let years = (to - from).num_days() as f64 / DAYS_PER_YEAR;
        // Simple interest never earns on itself, so it runs on the principal.
        let interest = if simple {
            principal * rate_pct / 100.0 * years
        } else {
            grow(balance, rate_pct, compounding, years) - balance
        };
        balance += interest;
        // Interest earned since the last year end is still all there until
        // the bank books it.
        if to == year_end || to == maturity {
            balance -= interest * tds;
            withheld += interest * tds;
        }
        from = to;
    }
    (balance, withheld)
}

/// The first 31 March after `date`.
fn financial_year_end(date: NaiveDate) -> NaiveDate {
    use chrono::Datelike;
    let this_year = NaiveDate::from_ymd_opt(date.year(), 3, 31).unwrap();
    if date < this_year {
        this_year
    } else {
        NaiveDate::from_ymd_opt(date.year() + 1, 3, 31).unwrap()
    }
}

pub fn value_on(d: &deposit::Model, as_of: NaiveDate) -> DepositValuation {
    let kind = DepositKind::from_str(&d.kind).unwrap_or(DepositKind::FixedDeposit);
    let compounding = Compounding::from_str(&d.compounding).unwrap_or(Compounding::Quarterly);
    let payout = PayoutFrequency::from_str(&d.payout_frequency).unwrap_or(PayoutFrequency::Cumulative);

    let term_days = (d.maturity_date - d.start_date).num_days().max(0);
    // Past maturity, stop accruing - the money is sitting in the bank now.
    let effective_date = as_of.min(d.maturity_date);
    let days_elapsed = (effective_date - d.start_date).num_days().max(0);
    let days_to_maturity = (d.maturity_date - as_of).num_days();

    let years_elapsed = days_elapsed as f64 / DAYS_PER_YEAR;
    let years_term = term_days as f64 / DAYS_PER_YEAR;

    let cumulative = matches!(payout, PayoutFrequency::Cumulative);
    let tds = d.tds_pct.filter(|p| *p > 0.0).map_or(0.0, |p| p.min(100.0) / 100.0);

    // Recurring deposits take no tax: each instalment is its own small
    // deposit, and withholding across them is not worth modelling.
    let (current_value, maturity_calc, principal_invested, interest_paid_out, tax_withheld, tax_at_maturity) = match kind {
        DepositKind::RecurringDeposit => {
            let inst = d.installment.unwrap_or(0.0);
            let (now_val, paid_now) =
                grow_recurring(inst, d.interest_rate, compounding, d.start_date, effective_date);
            let (mat_val, _) =
                grow_recurring(inst, d.interest_rate, compounding, d.start_date, d.maturity_date);
            (now_val, mat_val, paid_now, 0.0, 0.0, 0.0)
        }
        _ if cumulative => {
            let grow_to = |until: NaiveDate| {
                grow_withholding(d.principal, d.interest_rate, compounding, d.start_date, until, d.maturity_date, tds)
            };
            let (now_val, withheld) = grow_to(effective_date);
            let (mat_net, withheld_total) = grow_to(d.maturity_date);
            // Gross at maturity: the interest withheld was still earned.
            (now_val, mat_net + withheld_total, d.principal, 0.0, withheld, withheld_total)
        }
        // Interest leaves the deposit each period, so the balance never grows;
        // what has been paid out is tracked separately.
        _ => {
            let per_year = payout_periods_per_year(payout);
            let periods_done = (years_elapsed * per_year).floor();
            let periods_term = (years_term * per_year).floor();
            let per_period = d.principal * (d.interest_rate / 100.0) / per_year;
            (
                d.principal,
                d.principal,
                d.principal,
                periods_done * per_period * (1.0 - tds),
                periods_done * per_period * tds,
                periods_term * per_period * tds,
            )
        }
    };

    // The bank's own maturity figure wins when we have it.
    let (maturity_value, bank_quoted) = match d.maturity_amount {
        Some(v) if v > 0.0 => (v, true),
        _ => (maturity_calc, false),
    };
    // A payout deposit's tax went with its payouts; only a cumulative
    // deposit's comes out of what is paid back.
    let maturity_payout = if cumulative && kind != DepositKind::RecurringDeposit {
        maturity_value - tax_at_maturity
    } else {
        maturity_value
    };

    let accrued = current_value - principal_invested;
    let effective_yield = if principal_invested > 0.0 && years_term > 1e-6 && cumulative {
        ((maturity_value / principal_invested).powf(1.0 / years_term) - 1.0) * 100.0
    } else {
        d.interest_rate
    };

    DepositValuation {
        deposit_id: d.id,
        currency: d.currency_code.clone(),
        principal_invested: round_money(principal_invested),
        current_value: round_money(current_value),
        accrued_interest: round_money(accrued),
        interest_paid_out: round_money(interest_paid_out),
        maturity_value: round_money(maturity_value),
        interest_at_maturity: round_money(maturity_value - principal_invested),
        tax_withheld: round_money(tax_withheld),
        tax_at_maturity: round_money(tax_at_maturity),
        maturity_payout: round_money(maturity_payout),
        days_elapsed,
        days_to_maturity,
        term_days,
        progress_pct: if term_days > 0 {
            (days_elapsed as f64 / term_days as f64 * 100.0).clamp(0.0, 100.0)
        } else {
            100.0
        },
        effective_annual_yield: (effective_yield * 10_000.0).round() / 10_000.0,
        is_matured: as_of >= d.maturity_date,
        maturity_is_bank_quoted: bank_quoted,
    }
}

fn payout_periods_per_year(p: PayoutFrequency) -> f64 {
    match p {
        PayoutFrequency::Monthly => 12.0,
        PayoutFrequency::Quarterly => 4.0,
        PayoutFrequency::HalfYearly => 2.0,
        PayoutFrequency::Annual | PayoutFrequency::Cumulative => 1.0,
    }
}

fn months_between(from: NaiveDate, to: NaiveDate) -> u32 {
    use chrono::Datelike;
    if to <= from {
        return 0;
    }
    let months = (to.year() - from.year()) * 12 + (to.month() as i32 - from.month() as i32);
    months.max(0) as u32
}

/// Month arithmetic that clamps to the end of short months, so a deposit
/// started on the 31st does not skip February.
fn add_months(date: NaiveDate, months: u32) -> NaiveDate {
    use chrono::Datelike;
    let total = date.month0() + months;
    let year = date.year() + (total / 12) as i32;
    let month = total % 12 + 1;
    let day = date.day().min(days_in_month(year, month));
    NaiveDate::from_ymd_opt(year, month, day).unwrap_or(date)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    let first_next = NaiveDate::from_ymd_opt(ny, nm, 1).unwrap();
    let first_this = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    (first_next - first_this).num_days() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn fd(principal: f64, rate: f64, compounding: &str, payout: &str) -> deposit::Model {
        deposit::Model {
            id: 1,
            account_id: 1,
            kind: "FIXED_DEPOSIT".into(),
            institution: "HDFC Bank".into(),
            reference: None,
            principal,
            interest_rate: rate,
            compounding: compounding.into(),
            payout_frequency: payout.into(),
            installment: None,
            start_date: d(2024, 1, 1),
            maturity_date: d(2027, 1, 1),
            currency_code: "INR".into(),
            country: "IN".into(),
            auto_renew: false,
            status: "ACTIVE".into(),
            maturity_amount: None,
            tds_pct: None,
            notes: None,
            created_at: chrono::Utc::now().naive_utc(),
            updated_at: chrono::Utc::now().naive_utc(),
        }
    }

    #[test]
    fn quarterly_compounding_beats_simple_interest() {
        let simple = grow(100_000.0, 7.0, Compounding::Simple, 3.0);
        let quarterly = grow(100_000.0, 7.0, Compounding::Quarterly, 3.0);
        assert!((simple - 121_000.0).abs() < 1.0, "got {simple}");
        // 100000 * (1 + 0.07/4)^12 = 123144.xx
        assert!((quarterly - 123_144.0).abs() < 5.0, "got {quarterly}");
        assert!(quarterly > simple);
    }

    #[test]
    fn a_cumulative_fd_accrues_toward_its_maturity_value() {
        let v = value_on(&fd(100_000.0, 7.0, "QUARTERLY", "CUMULATIVE"), d(2025, 7, 1));
        assert!(v.current_value > 100_000.0);
        assert!(v.current_value < v.maturity_value);
        assert!((v.progress_pct - 50.0).abs() < 2.0, "got {}", v.progress_pct);
        assert!(!v.is_matured);
        assert!(v.effective_annual_yield > 7.0, "compounding lifts the headline rate");
    }

    #[test]
    fn a_payout_fd_stays_at_principal_and_tracks_what_it_paid_away() {
        let v = value_on(&fd(100_000.0, 8.0, "QUARTERLY", "QUARTERLY"), d(2025, 1, 1));
        assert_eq!(v.current_value, 100_000.0);
        assert_eq!(v.accrued_interest, 0.0);
        // Four quarters at 2000 each.
        assert!((v.interest_paid_out - 8_000.0).abs() < 1.0, "got {}", v.interest_paid_out);
    }

    #[test]
    fn accrual_stops_at_maturity() {
        let deposit = fd(100_000.0, 7.0, "QUARTERLY", "CUMULATIVE");
        let at_maturity = value_on(&deposit, d(2027, 1, 1));
        let long_after = value_on(&deposit, d(2030, 1, 1));
        assert_eq!(at_maturity.current_value, long_after.current_value);
        assert!(long_after.is_matured);
        assert!(long_after.days_to_maturity < 0);
    }

    #[test]
    fn a_bank_quoted_maturity_overrides_our_arithmetic() {
        let mut deposit = fd(100_000.0, 7.0, "QUARTERLY", "CUMULATIVE");
        deposit.maturity_amount = Some(123_000.0);
        let v = value_on(&deposit, d(2025, 1, 1));
        assert_eq!(v.maturity_value, 123_000.0);
        assert!(v.maturity_is_bank_quoted);
    }

    #[test]
    fn a_recurring_deposit_counts_only_instalments_paid_so_far() {
        let mut rd = fd(0.0, 7.0, "QUARTERLY", "CUMULATIVE");
        rd.kind = "RECURRING_DEPOSIT".into();
        rd.installment = Some(5_000.0);
        rd.maturity_date = d(2025, 1, 1);

        let half = value_on(&rd, d(2024, 7, 1));
        assert!((half.principal_invested - 30_000.0).abs() < 1.0, "got {}", half.principal_invested);

        let full = value_on(&rd, d(2025, 1, 1));
        assert!((full.principal_invested - 60_000.0).abs() < 1.0, "got {}", full.principal_invested);
        assert!(full.current_value > full.principal_invested);
    }

    /// An ICICI NRO deposit: Rs 25,000 at 7.25% for two years, 31.2% TDS.
    /// ICICI's statement: 1,785 interest booked at 31 March 2026, a balance
    /// of 26,228 after tax, and 28,820 at maturity with 3,820 interest.
    fn nro() -> deposit::Model {
        let mut dep = fd(25_000.0, 7.25, "QUARTERLY", "CUMULATIVE");
        dep.start_date = d(2025, 4, 16);
        dep.maturity_date = d(2027, 4, 16);
        dep.tds_pct = Some(31.2);
        dep
    }

    #[test]
    fn tds_comes_out_of_the_deposit_at_the_year_end() {
        let v = value_on(&nro(), d(2026, 4, 1));
        assert!((v.tax_withheld - 554.7).abs() < 1.0, "got {}", v.tax_withheld);
        assert!((v.current_value - 26_223.0).abs() < 10.0, "the bank shows 26,228; got {}", v.current_value);

        // Mid-year, this year's interest is all still in the deposit.
        let today = value_on(&nro(), d(2026, 10, 2));
        assert_eq!(today.tax_withheld, v.tax_withheld);
        assert!((today.current_value - 27_196.0).abs() < 5.0, "got {}", today.current_value);
    }

    #[test]
    fn the_gross_maturity_matches_the_banks_and_the_payout_is_less_the_tax() {
        let mut deposit = nro();
        let calc = value_on(&deposit, d(2026, 10, 2));
        assert!((calc.maturity_value - 28_820.0).abs() < 5.0, "the bank quotes 28,820; got {}", calc.maturity_value);
        assert!((calc.tax_at_maturity - 1_191.3).abs() < 1.0, "got {}", calc.tax_at_maturity);
        assert!((calc.maturity_payout - 27_627.0).abs() < 2.0, "got {}", calc.maturity_payout);

        deposit.maturity_amount = Some(28_820.0);
        let quoted = value_on(&deposit, d(2026, 10, 2));
        assert_eq!(quoted.interest_at_maturity, 3_820.0);
        assert!((quoted.maturity_payout - (28_820.0 - quoted.tax_at_maturity)).abs() < 0.01);
    }

    #[test]
    fn without_tds_the_year_end_changes_nothing() {
        let mut deposit = nro();
        deposit.tds_pct = None;
        let v = value_on(&deposit, d(2026, 10, 2));
        let straight = grow(25_000.0, 7.25, Compounding::Quarterly, 534.0 / DAYS_PER_YEAR);
        assert!((v.current_value - straight).abs() < 0.01, "{} vs {straight}", v.current_value);
        assert_eq!(v.tax_withheld, 0.0);
        assert_eq!(v.maturity_payout, v.maturity_value);
    }

    #[test]
    fn a_payout_deposit_pays_its_interest_out_after_tax() {
        let mut deposit = fd(100_000.0, 8.0, "QUARTERLY", "QUARTERLY");
        deposit.tds_pct = Some(10.0);
        let v = value_on(&deposit, d(2025, 1, 1));
        assert_eq!(v.current_value, 100_000.0);
        assert!((v.interest_paid_out - 7_200.0).abs() < 1.0, "got {}", v.interest_paid_out);
        assert!((v.tax_withheld - 800.0).abs() < 1.0, "got {}", v.tax_withheld);
        assert_eq!(v.maturity_payout, 100_000.0);
    }

    #[test]
    fn a_brand_new_deposit_is_worth_exactly_its_principal() {
        let v = value_on(&fd(50_000.0, 6.5, "QUARTERLY", "CUMULATIVE"), d(2024, 1, 1));
        assert_eq!(v.current_value, 50_000.0);
        assert_eq!(v.accrued_interest, 0.0);
        assert_eq!(v.progress_pct, 0.0);
    }

    #[test]
    fn month_arithmetic_clamps_to_short_months() {
        assert_eq!(add_months(d(2024, 1, 31), 1), d(2024, 2, 29));
        assert_eq!(add_months(d(2023, 1, 31), 1), d(2023, 2, 28));
        assert_eq!(add_months(d(2024, 11, 30), 3), d(2025, 2, 28));
    }
}
