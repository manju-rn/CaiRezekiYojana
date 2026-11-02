//! Money-weighted return (XIRR) - the number that actually reflects a SIP.
//!
//! Newton-Raphson converges in a handful of iterations for well-behaved
//! portfolios but wanders off for irregular flows, so a bracketing bisection
//! backs it up.

use chrono::NaiveDate;

use super::holdings::CashFlow;

const DAYS_PER_YEAR: f64 = 365.0;
const MAX_NEWTON_ITERS: usize = 64;
const MAX_BISECT_ITERS: usize = 200;
const TOLERANCE: f64 = 1e-9;
/// Below -99.999% the discount factor underflows; above 1000x/yr is noise.
const RATE_FLOOR: f64 = -0.999_99;
const RATE_CEIL: f64 = 1000.0;

/// Net present value of `flows` at annual rate `rate`, discounted from the
/// first flow's date.
fn npv(flows: &[CashFlow], t0: NaiveDate, rate: f64) -> f64 {
    flows
        .iter()
        .map(|f| {
            let years = (f.date - t0).num_days() as f64 / DAYS_PER_YEAR;
            f.amount / (1.0 + rate).powf(years)
        })
        .sum()
}

fn dnpv(flows: &[CashFlow], t0: NaiveDate, rate: f64) -> f64 {
    flows
        .iter()
        .map(|f| {
            let years = (f.date - t0).num_days() as f64 / DAYS_PER_YEAR;
            -years * f.amount / (1.0 + rate).powf(years + 1.0)
        })
        .sum()
}

/// Annualised money-weighted return as a percentage, e.g. `12.4` for 12.4%.
///
/// Returns `None` when the flows cannot define a return: fewer than two
/// flows, all the same sign, or no root in a sane range.
pub fn xirr(flows: &[CashFlow]) -> Option<f64> {
    if flows.len() < 2 {
        return None;
    }

    // Without both an outflow and an inflow there is nothing to solve for.
    let has_negative = flows.iter().any(|f| f.amount < 0.0);
    let has_positive = flows.iter().any(|f| f.amount > 0.0);
    if !has_negative || !has_positive {
        return None;
    }

    let mut sorted = flows.to_vec();
    sorted.sort_by_key(|f| f.date);
    let t0 = sorted[0].date;

    // Every flow on one day means zero elapsed time - no rate is meaningful.
    if sorted.last()?.date == t0 {
        return None;
    }

    if let Some(rate) = newton(&sorted, t0) {
        return Some(rate * 100.0);
    }
    bisect(&sorted, t0).map(|r| r * 100.0)
}

fn newton(flows: &[CashFlow], t0: NaiveDate) -> Option<f64> {
    let mut rate = 0.1;
    for _ in 0..MAX_NEWTON_ITERS {
        let value = npv(flows, t0, rate);
        if value.abs() < TOLERANCE {
            return Some(rate);
        }
        let slope = dnpv(flows, t0, rate);
        if slope.abs() < TOLERANCE {
            return None;
        }
        let next = rate - value / slope;
        if !next.is_finite() || next <= RATE_FLOOR || next > RATE_CEIL {
            return None;
        }
        if (next - rate).abs() < TOLERANCE {
            return Some(next);
        }
        rate = next;
    }
    None
}

fn bisect(flows: &[CashFlow], t0: NaiveDate) -> Option<f64> {
    let (mut lo, mut hi) = (RATE_FLOOR, RATE_CEIL);
    let (mut f_lo, f_hi) = (npv(flows, t0, lo), npv(flows, t0, hi));
    if f_lo * f_hi > 0.0 {
        return None;
    }
    for _ in 0..MAX_BISECT_ITERS {
        let mid = (lo + hi) / 2.0;
        let f_mid = npv(flows, t0, mid);
        if f_mid.abs() < TOLERANCE || (hi - lo).abs() < TOLERANCE {
            return Some(mid);
        }
        if f_lo * f_mid < 0.0 {
            hi = mid;
        } else {
            lo = mid;
            f_lo = f_mid;
        }
    }
    None
}

/// Simple absolute return, for positions too short-lived for XIRR to mean much.
pub fn absolute_return_pct(invested: f64, current: f64) -> Option<f64> {
    if invested.abs() < f64::EPSILON {
        None
    } else {
        Some((current - invested) / invested.abs() * 100.0)
    }
}

/// Convert an absolute return into an annualised (CAGR) figure.
pub fn annualise(invested: f64, current: f64, days: i64) -> Option<f64> {
    if invested <= 0.0 || current <= 0.0 || days <= 0 {
        return None;
    }
    let years = days as f64 / DAYS_PER_YEAR;
    if years < 1e-6 {
        return None;
    }
    Some(((current / invested).powf(1.0 / years) - 1.0) * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn cf(date: NaiveDate, amount: f64) -> CashFlow {
        CashFlow { date, amount }
    }

    #[test]
    fn doubling_over_one_year_is_one_hundred_percent() {
        let r = xirr(&[cf(d(2024, 1, 1), -1000.0), cf(d(2025, 1, 1), 2000.0)]).unwrap();
        assert!((r - 100.0).abs() < 0.5, "got {r}");
    }

    #[test]
    fn flat_return_is_zero() {
        let r = xirr(&[cf(d(2024, 1, 1), -1000.0), cf(d(2025, 1, 1), 1000.0)]).unwrap();
        assert!(r.abs() < 0.01, "got {r}");
    }

    #[test]
    fn a_monthly_sip_annualises_sensibly() {
        let mut flows: Vec<CashFlow> = (0..12)
            .map(|i| cf(d(2024, 1 + i as u32, 1), -10_000.0))
            .collect();
        flows.push(cf(d(2025, 1, 1), 126_000.0));
        let r = xirr(&flows).unwrap();
        // ~5% simple on an average 6-month holding reads as ~10% annualised.
        assert!(r > 8.0 && r < 13.0, "got {r}");
    }

    #[test]
    fn a_loss_reports_negative() {
        let r = xirr(&[cf(d(2024, 1, 1), -1000.0), cf(d(2025, 1, 1), 800.0)]).unwrap();
        assert!(r < -15.0 && r > -25.0, "got {r}");
    }

    #[test]
    fn undefined_cases_return_none() {
        assert!(xirr(&[]).is_none());
        assert!(xirr(&[cf(d(2024, 1, 1), -1000.0)]).is_none());
        // All outflows - never any money back.
        assert!(xirr(&[cf(d(2024, 1, 1), -100.0), cf(d(2024, 6, 1), -100.0)]).is_none());
        // Same-day flows leave no time for a rate to act over.
        assert!(xirr(&[cf(d(2024, 1, 1), -100.0), cf(d(2024, 1, 1), 110.0)]).is_none());
    }

    #[test]
    fn cagr_matches_a_known_doubling() {
        let r = annualise(1000.0, 2000.0, 365).unwrap();
        assert!((r - 100.0).abs() < 0.5, "got {r}");
    }
}
