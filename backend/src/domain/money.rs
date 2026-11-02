//! Money that always travels with its currency, and - once converted - with
//! the exact rate used.

use serde::{Deserialize, Serialize};

/// An amount in a single currency.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Money {
    pub amount: f64,
    pub currency: String,
}

impl Money {
    pub fn new(amount: f64, currency: impl Into<String>) -> Self {
        Self { amount, currency: currency.into() }
    }

    pub fn zero(currency: impl Into<String>) -> Self {
        Self::new(0.0, currency)
    }
}

/// An amount shown in its own currency *and* in the portfolio base currency,
/// with the rate that got it there.
///
/// This is the shape the UI renders for every non-base holding: the native
/// figure, the exchange number, and the converted figure - so a number is
/// never silently re-based without the rate being visible next to it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ConvertedMoney {
    /// Amount in the instrument's / account's own currency.
    pub amount: f64,
    pub currency: String,
    /// Units of `base_currency` per 1 `currency`, after margin. `1.0` when
    /// `currency == base_currency`.
    pub fx_rate: f64,
    pub base_amount: f64,
    pub base_currency: String,
    /// `identity`, `manual`, `xe`, `frankfurter`, `exchangerate_host`,
    /// `inverse:<source>`, `cross:<pivot>:<source>`, or `stale`.
    pub fx_source: String,
    /// Date the underlying provider rate was published.
    pub fx_as_of: Option<chrono::NaiveDate>,
    /// True when a margin or manual override moved the rate off the raw feed.
    pub fx_adjusted: bool,
    /// The transfer fee taken off, as a percentage of the amount, where the
    /// pair charges one by amount.
    pub fx_fee_pct: Option<f64>,
}

impl ConvertedMoney {
    /// Same-currency case: no conversion, rate pinned at 1.
    pub fn identity(amount: f64, currency: impl Into<String>) -> Self {
        let c = currency.into();
        Self {
            amount,
            currency: c.clone(),
            fx_rate: 1.0,
            base_amount: amount,
            base_currency: c,
            fx_source: "identity".to_string(),
            fx_as_of: None,
            fx_adjusted: false,
            fx_fee_pct: None,
        }
    }
}

/// Round for display. Intermediate maths stays full-precision; only outputs
/// crossing into a report or the UI go through here.
pub fn round_to(value: f64, places: u32) -> f64 {
    let f = 10f64.powi(places as i32);
    (value * f).round() / f
}

pub fn round_money(value: f64) -> f64 {
    round_to(value, 2)
}

/// Units/shares keep more precision - Indian MF units routinely carry three
/// or four decimals.
pub fn round_qty(value: f64) -> f64 {
    round_to(value, 6)
}

/// Percentage change guarded against a zero cost base.
pub fn pct_change(from: f64, to: f64) -> Option<f64> {
    if from.abs() < f64::EPSILON {
        None
    } else {
        Some(round_to((to - from) / from.abs() * 100.0, 4))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_half_away_from_zero() {
        assert_eq!(round_money(1.005), 1.01);
        assert_eq!(round_money(-1.005), -1.01);
        assert_eq!(round_qty(0.1234567), 0.123457);
    }

    #[test]
    fn pct_change_guards_zero_base() {
        assert_eq!(pct_change(0.0, 10.0), None);
        assert_eq!(pct_change(200.0, 250.0), Some(25.0));
        // A short/negative cost base still reports a sane direction.
        assert_eq!(pct_change(-100.0, -50.0), Some(50.0));
    }
}
