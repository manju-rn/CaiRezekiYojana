//! Bank and savings balances across India, Singapore and Australia.
//!
//! Balances are snapshots rather than a ledger: the most recent row for an
//! account is its current balance. That matches how people actually track
//! this - you check the app and type in what it says.

use chrono::NaiveDate;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

use crate::domain::fx::{DisplayTotals, Equivalents, FxEngine, TotalRates};
use crate::domain::money::round_money;
use crate::entities::{account, cash_balance};
use crate::error::AppResult;
use crate::types::AccountKind;

use super::portfolio::FxView;

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CashAccountView {
    pub account_id: i32,
    pub name: String,
    pub institution: Option<String>,
    pub country: String,
    pub currency: String,
    pub balance: f64,
    pub as_of: Option<NaiveDate>,
    /// True when the newest snapshot is more than 30 days old.
    pub is_stale: bool,
    pub fx: FxView,
    pub balance_base: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CashSummary {
    pub base_currency: String,
    pub total_base: f64,
    /// The currencies `total_values` carries, in display order.
    pub display_currencies: Vec<String>,
    /// The total in each display currency, each balance converted from its
    /// own currency - so a Singapore account counts at face value in SGD.
    pub total_values: BTreeMap<String, Option<f64>>,
    pub accounts: usize,
    pub by_currency: Vec<CashCurrencyBucket>,
    /// Balances per bank, largest first.
    pub by_institution: Vec<BankSplit>,
    pub stale_accounts: usize,
}

/// One bank's share of a page's total, in the base and in every display
/// currency. Shared by the cash and deposits pages.
#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BankSplit {
    pub institution: String,
    /// Accounts on the cash page, deposits on the deposits page.
    pub count: usize,
    pub value_base: f64,
    pub weight_pct: f64,
    /// Keyed by display currency; `None` where part of it has no rate.
    pub values: BTreeMap<String, Option<f64>>,
}

/// Where an account or deposit with no institution recorded is grouped.
pub const NO_INSTITUTION: &str = "No institution recorded";

/// Group `(institution, currency, amount)` by bank. Every bank uses the
/// page's rates, so each column adds up to the page's total.
pub fn split_by_bank<'a>(
    fx: &FxEngine,
    rows: impl IntoIterator<Item = (Option<&'a str>, &'a str, f64)>,
) -> Vec<BankSplit> {
    let rows: Vec<(String, &str, f64)> = rows
        .into_iter()
        .map(|(i, c, a)| {
            let name = i.map(str::trim).filter(|s| !s.is_empty()).unwrap_or(NO_INSTITUTION);
            (name.to_string(), c, a)
        })
        .collect();
    let rates = TotalRates::new(fx, rows.iter().map(|(_, c, a)| (*c, *a)));
    let display = DisplayTotals::new(fx, rows.iter().map(|(_, c, a)| (*c, *a)));

    let mut banks: BTreeMap<&str, Vec<(&str, f64)>> = BTreeMap::new();
    for (name, c, a) in &rows {
        banks.entry(name.as_str()).or_default().push((c, *a));
    }
    let total: f64 = rows.iter().map(|(_, c, a)| a * rates.rate(c)).sum();
    let mut out: Vec<BankSplit> = banks
        .into_iter()
        .map(|(name, parts)| {
            let value = parts.iter().map(|(c, a)| a * rates.rate(c)).sum::<f64>();
            BankSplit {
                institution: name.to_string(),
                count: parts.len(),
                value_base: round_money(value),
                weight_pct: if total.abs() > f64::EPSILON { round_money(value / total * 100.0) } else { 0.0 },
                values: display.values(parts.iter().copied()),
            }
        })
        .collect();
    out.sort_by(|a, b| b.value_base.partial_cmp(&a.value_base).unwrap_or(std::cmp::Ordering::Equal));
    out
}

#[derive(Clone, Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CashCurrencyBucket {
    pub currency: String,
    pub country: String,
    pub balance: f64,
    pub fx_rate: f64,
    pub balance_base: f64,
    pub accounts: usize,
}

/// A snapshot older than this is probably not what is really in the account.
const STALE_AFTER_DAYS: i64 = 30;

/// `owner`'s bank accounts, each at its latest balance.
pub async fn list(
    db: &DatabaseConnection,
    owner: i32,
    fx: &FxEngine,
    today: NaiveDate,
) -> AppResult<Vec<CashAccountView>> {
    let accounts = account::Entity::find().filter(account::Column::UserId.eq(owner)).all(db).await?;
    let ids: Vec<i32> = accounts.iter().map(|a| a.id).collect();

    // Ascending so the final write per account is its newest snapshot.
    let mut latest: HashMap<i32, cash_balance::Model> = HashMap::new();
    for b in cash_balance::Entity::find()
        .filter(cash_balance::Column::AccountId.is_in(ids))
        .order_by_asc(cash_balance::Column::AsOf)
        .order_by_asc(cash_balance::Column::Id)
        .all(db)
        .await?
    {
        latest.insert(b.account_id, b);
    }

    let mut out = Vec::new();
    for a in accounts {
        // Deposit containers are valued through the deposits module, not here,
        // or the same money would be counted twice in net worth.
        if a.kind != AccountKind::Bank.as_str() {
            continue;
        }

        let snapshot = latest.get(&a.id);
        let balance = snapshot.map(|s| s.balance).unwrap_or(0.0);
        let currency = snapshot
            .map(|s| s.currency_code.clone())
            .unwrap_or_else(|| a.currency_code.clone());
        // Priced on the whole balance, as one conversion.
        let converted = fx.to_base_or_flag(balance, &currency);

        out.push(CashAccountView {
            account_id: a.id,
            name: a.name.clone(),
            institution: a.institution.clone(),
            country: a.country.clone(),
            currency,
            balance: round_money(balance),
            as_of: snapshot.map(|s| s.as_of),
            is_stale: snapshot
                .map(|s| (today - s.as_of).num_days() > STALE_AFTER_DAYS)
                .unwrap_or(true),
            balance_base: round_money(converted.base_amount),
            fx: FxView::of(&converted),
        });
    }

    out.sort_by(|a, b| {
        b.balance_base
            .partial_cmp(&a.balance_base)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

pub fn summarise(views: &[CashAccountView], base: &str, fx: &FxEngine) -> CashSummary {
    // Totals are priced per currency, as one conversion of every balance
    // held in it, so a fee charged by amount is worked out on the whole.
    let rates = TotalRates::new(fx, views.iter().map(|v| (v.currency.as_str(), v.balance)));
    let mut buckets: HashMap<String, CashCurrencyBucket> = HashMap::new();
    let mut total = Equivalents::default();
    for v in views {
        total.add(v.balance, &v.currency);
        let b = buckets.entry(v.currency.clone()).or_insert(CashCurrencyBucket {
            currency: v.currency.clone(),
            country: v.country.clone(),
            balance: 0.0,
            fx_rate: rates.rate(&v.currency),
            balance_base: 0.0,
            accounts: 0,
        });
        b.balance += v.balance;
        b.balance_base += v.balance * rates.rate(&v.currency);
        b.accounts += 1;
    }
    let mut by_currency: Vec<CashCurrencyBucket> = buckets.into_values().collect();
    for b in by_currency.iter_mut() {
        b.balance = round_money(b.balance);
        b.balance_base = round_money(b.balance_base);
    }
    by_currency.sort_by(|a, b| {
        b.balance_base
            .partial_cmp(&a.balance_base)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    CashSummary {
        base_currency: base.to_string(),
        total_base: round_money(views.iter().map(|v| v.balance * rates.rate(&v.currency)).sum()),
        display_currencies: fx.display_currencies(),
        total_values: total.rounded(fx),
        accounts: views.len(),
        stale_accounts: views.iter().filter(|v| v.is_stale).count(),
        by_currency,
        by_institution: split_by_bank(
            fx,
            views.iter().map(|v| (v.institution.as_deref(), v.currency.as_str(), v.balance)),
        ),
    }
}
