//! Deposit views: the stored deposit, its computed valuation, and the
//! base-currency conversion with the rate attached.

use chrono::NaiveDate;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

use crate::domain::deposits::{value_on, DepositValuation};
use crate::domain::fx::{DisplayTotals, FxEngine, TotalRates};
use crate::domain::money::round_money;
use crate::entities::{account, deposit};
use crate::error::AppResult;

use super::cash::{split_by_bank, BankSplit};
use super::portfolio::FxView;

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DepositView {
    #[serde(flatten)]
    pub deposit: deposit::Model,
    pub account_name: String,
    pub valuation: DepositValuation,
    pub fx: FxView,
    pub current_value_base: f64,
    pub principal_base: f64,
    pub maturity_value_base: f64,
    /// What comes back at maturity, after tax withheld.
    pub maturity_payout_base: f64,
    pub accrued_interest_base: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DepositSummary {
    pub base_currency: String,
    pub count: usize,
    pub active_count: usize,
    pub principal_base: f64,
    pub current_value_base: f64,
    pub accrued_interest_base: f64,
    pub maturity_value_base: f64,
    /// What comes back at maturity, after tax withheld at source.
    pub maturity_payout_base: f64,
    /// Tax withheld at source so far, across every deposit.
    pub tax_withheld_base: f64,
    /// Weighted by principal, so a large deposit moves it more than a small one.
    pub weighted_average_rate: f64,
    /// The currencies `current_values` carries, in display order.
    pub display_currencies: Vec<String>,
    /// Current value in each display currency, each deposit converted from
    /// its own currency.
    pub current_values: BTreeMap<String, Option<f64>>,
    pub by_currency: Vec<DepositCurrencyBucket>,
    /// Current value per bank, largest first.
    pub by_institution: Vec<BankSplit>,
    /// Active deposits maturing within 90 days, soonest first.
    pub maturing_soon: Vec<MaturingDeposit>,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct DepositCurrencyBucket {
    pub currency: String,
    pub country: String,
    pub count: usize,
    pub principal: f64,
    pub current_value: f64,
    pub fx_rate: f64,
    pub current_value_base: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct MaturingDeposit {
    pub deposit_id: i32,
    pub institution: String,
    pub currency: String,
    pub maturity_date: NaiveDate,
    pub days_to_maturity: i64,
    pub maturity_value: f64,
    pub maturity_value_base: f64,
    pub auto_renew: bool,
}

const MATURING_SOON_DAYS: i64 = 90;

/// `owner`'s deposits, each valued on `as_of`.
pub async fn list(
    db: &DatabaseConnection,
    owner: i32,
    fx: &FxEngine,
    as_of: NaiveDate,
) -> AppResult<Vec<DepositView>> {
    let accounts: HashMap<i32, account::Model> = account::Entity::find()
        .filter(account::Column::UserId.eq(owner))
        .all(db)
        .await?
        .into_iter()
        .map(|a| (a.id, a))
        .collect();

    let mut out = Vec::new();
    let ids: Vec<i32> = accounts.keys().copied().collect();
    for d in deposit::Entity::find().filter(deposit::Column::AccountId.is_in(ids)).all(db).await? {
        let valuation = value_on(&d, as_of);
        // One rate for the whole deposit, priced on what it is worth today.
        let converted = fx.to_base_or_flag(valuation.current_value, &d.currency_code);
        let rate = converted.fx_rate;

        out.push(DepositView {
            account_name: accounts
                .get(&d.account_id)
                .map(|a| a.name.clone())
                .unwrap_or_else(|| "(unknown account)".to_string()),
            fx: FxView::of(&converted),
            current_value_base: round_money(valuation.current_value * rate),
            principal_base: round_money(valuation.principal_invested * rate),
            maturity_value_base: round_money(valuation.maturity_value * rate),
            maturity_payout_base: round_money(valuation.maturity_payout * rate),
            accrued_interest_base: round_money(valuation.accrued_interest * rate),
            valuation,
            deposit: d,
        });
    }

    // Soonest maturity first - the list is mostly used to decide what to roll.
    out.sort_by_key(|v| v.deposit.maturity_date);
    Ok(out)
}

pub fn summarise(views: &[DepositView], base: &str, as_of: NaiveDate, fx: &FxEngine) -> DepositSummary {
    // Totals are priced per currency, as one conversion of everything held
    // in it, so a fee charged by amount is worked out on the whole.
    let rates = TotalRates::new(
        fx,
        views.iter().map(|v| (v.deposit.currency_code.as_str(), v.valuation.current_value)),
    );
    let in_base = |v: &DepositView, amount: f64| amount * rates.rate(&v.deposit.currency_code);
    let current = views.iter().map(|v| (v.deposit.currency_code.as_str(), v.valuation.current_value));

    let active: Vec<&DepositView> = views
        .iter()
        .filter(|v| !v.valuation.is_matured && v.deposit.status == "ACTIVE")
        .collect();

    let principal_weight: f64 = views.iter().map(|v| v.valuation.principal_invested).sum();
    let weighted_rate = if principal_weight > 0.0 {
        views
            .iter()
            .map(|v| v.deposit.interest_rate * v.valuation.principal_invested)
            .sum::<f64>()
            / principal_weight
    } else {
        0.0
    };

    let mut buckets: HashMap<String, DepositCurrencyBucket> = HashMap::new();
    for v in views {
        let b = buckets
            .entry(v.deposit.currency_code.clone())
            .or_insert(DepositCurrencyBucket {
                currency: v.deposit.currency_code.clone(),
                country: v.deposit.country.clone(),
                count: 0,
                principal: 0.0,
                current_value: 0.0,
                fx_rate: rates.rate(&v.deposit.currency_code),
                current_value_base: 0.0,
            });
        b.count += 1;
        b.principal += v.valuation.principal_invested;
        b.current_value += v.valuation.current_value;
        b.current_value_base += in_base(v, v.valuation.current_value);
    }
    let mut by_currency: Vec<DepositCurrencyBucket> = buckets.into_values().collect();
    for b in by_currency.iter_mut() {
        b.principal = round_money(b.principal);
        b.current_value = round_money(b.current_value);
        b.current_value_base = round_money(b.current_value_base);
    }
    by_currency.sort_by(|a, b| {
        b.current_value_base
            .partial_cmp(&a.current_value_base)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut maturing: Vec<MaturingDeposit> = active
        .iter()
        .filter(|v| {
            let days = (v.deposit.maturity_date - as_of).num_days();
            (0..=MATURING_SOON_DAYS).contains(&days)
        })
        .map(|v| MaturingDeposit {
            deposit_id: v.deposit.id,
            institution: v.deposit.institution.clone(),
            currency: v.deposit.currency_code.clone(),
            maturity_date: v.deposit.maturity_date,
            days_to_maturity: (v.deposit.maturity_date - as_of).num_days(),
            maturity_value: v.valuation.maturity_value,
            maturity_value_base: round_money(in_base(v, v.valuation.maturity_value)),
            auto_renew: v.deposit.auto_renew,
        })
        .collect();
    maturing.sort_by_key(|m| m.days_to_maturity);

    DepositSummary {
        base_currency: base.to_string(),
        count: views.len(),
        active_count: active.len(),
        principal_base: round_money(views.iter().map(|v| in_base(v, v.valuation.principal_invested)).sum()),
        current_value_base: round_money(views.iter().map(|v| in_base(v, v.valuation.current_value)).sum()),
        accrued_interest_base: round_money(
            views.iter().map(|v| in_base(v, v.valuation.accrued_interest)).sum(),
        ),
        maturity_value_base: round_money(views.iter().map(|v| in_base(v, v.valuation.maturity_value)).sum()),
        maturity_payout_base: round_money(views.iter().map(|v| in_base(v, v.valuation.maturity_payout)).sum()),
        tax_withheld_base: round_money(views.iter().map(|v| in_base(v, v.valuation.tax_withheld)).sum()),
        weighted_average_rate: (weighted_rate * 10_000.0).round() / 10_000.0,
        display_currencies: fx.display_currencies(),
        current_values: DisplayTotals::new(fx, current.clone()).values(current.clone()),
        by_currency,
        by_institution: split_by_bank(
            fx,
            views.iter().map(|v| {
                (Some(v.deposit.institution.as_str()), v.deposit.currency_code.as_str(), v.valuation.current_value)
            }),
        ),
        maturing_soon: maturing,
    }
}
