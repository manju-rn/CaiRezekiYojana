//! What Wise charges to convert, from the public pricing endpoint behind the
//! calculator on wise.com. No key needed.
//!
//! One request prices one amount on every route (bank transfer, card,
//! balance...). The fee depends on the amount, so a pair is sampled at
//! several and `FeeSchedule` interpolates between them.

use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

const DEFAULT_BASE: &str = "https://wise.com/gateway/v1/price";
pub const PROVIDER: &str = "wise";

/// Converting from one Wise balance into another: a percentage of the
/// amount, no flat fee.
const PAY_IN: &str = "BALANCE";
const PAY_OUT: &str = "BALANCE";
pub const ROUTE: &str = "BALANCE>BALANCE";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Price {
    pay_in_method: String,
    pay_out_method: String,
    /// Every fee on the route, in the source currency.
    total: f64,
    mid_rate: Option<f64>,
}

#[derive(Deserialize)]
struct Rejection {
    description: Option<String>,
}

/// The fee sampled at each amount, as `(amount, fee)` in `from`, and Wise's
/// mid-market rate for the pair.
pub struct Sampled {
    pub points: Vec<(f64, f64)>,
    pub mid_rate: Option<f64>,
}

pub async fn sample(state: &AppState, from: &str, to: &str, amounts: &[f64]) -> AppResult<Sampled> {
    let root = state
        .setting_opt("wise_price_url")
        .await
        .unwrap_or_else(|| DEFAULT_BASE.to_string());

    let mut points = Vec::with_capacity(amounts.len());
    let mut mid_rate = None;
    for &amount in amounts {
        let res = state
            .http
            .get(&root)
            .query(&[
                ("sourceAmount", format!("{amount:.0}")),
                ("sourceCurrency", from.to_string()),
                ("targetCurrency", to.to_string()),
            ])
            .send()
            .await?;
        let status = res.status();
        if !status.is_success() {
            // Wise explains a pair it does not serve, e.g. an unsupported source.
            let why = res
                .json::<Rejection>()
                .await
                .ok()
                .and_then(|r| r.description)
                .unwrap_or_else(|| format!("HTTP {status}"));
            return Err(AppError::provider(PROVIDER, format!("{from}/{to}: {why}")));
        }
        let prices: Vec<Price> = res.json().await?;
        let Some(p) = prices.iter().find(|p| p.pay_in_method == PAY_IN && p.pay_out_method == PAY_OUT) else {
            return Err(AppError::provider(
                PROVIDER,
                format!("{from}/{to}: no balance-to-balance price for {amount:.0} {from}"),
            ));
        };
        points.push((amount, p.total));
        mid_rate = mid_rate.or(p.mid_rate);
    }
    Ok(Sampled { points, mid_rate })
}
