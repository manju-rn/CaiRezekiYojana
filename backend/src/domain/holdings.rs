//! Turning a stream of transactions into positions.
//!
//! Positions are always derived, never stored. Re-importing a corrected
//! statement therefore fixes every downstream number for free, and there is
//! no cached position table to drift out of sync.

use std::collections::VecDeque;
use std::str::FromStr;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::entities::txn;
use crate::types::{CostBasisMethod, TxnKind};

use super::money::{pct_change, round_money, round_qty};

/// One purchase tranche still open, in the instrument's own currency.
#[derive(Clone, Copy, Debug)]
struct Lot {
    qty: f64,
    /// Acquisition cost per unit, fees and taxes folded in.
    cost_per_unit: f64,
    acquired: NaiveDate,
}

/// Quantities below this are float dust left over from lot arithmetic, not
/// real fractional shares.
const DUST: f64 = 1e-9;

#[derive(Clone, Debug, Default, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CostBasis {
    pub quantity: f64,
    /// Cost of the units still held.
    pub invested: f64,
    /// `invested / quantity`, or 0 for a closed position.
    pub average_cost: f64,
    pub realised_pnl: f64,
    /// Cash dividends received (excludes reinvested units, which show up as
    /// extra quantity instead).
    pub dividends: f64,
    pub fees_paid: f64,
    pub first_trade: Option<NaiveDate>,
    pub last_trade: Option<NaiveDate>,
}

/// A single cash movement, for XIRR.
#[derive(Clone, Copy, Debug)]
pub struct CashFlow {
    pub date: NaiveDate,
    /// Negative = money out of pocket, positive = money returned.
    pub amount: f64,
}

/// Walks transactions in trade order and maintains open lots.
pub struct LotBook {
    method: CostBasisMethod,
    lots: VecDeque<Lot>,
    basis: CostBasis,
    flows: Vec<CashFlow>,
}

impl LotBook {
    pub fn new(method: CostBasisMethod) -> Self {
        Self { method, lots: VecDeque::new(), basis: CostBasis::default(), flows: Vec::new() }
    }

    /// `txns` must be sorted by trade date then id. Unparseable kinds are
    /// skipped rather than aborting the whole position.
    pub fn apply_all(&mut self, txns: &[txn::Model]) {
        for t in txns {
            if let Ok(kind) = TxnKind::from_str(&t.kind) {
                self.apply(t, kind);
            }
        }
    }

    fn apply(&mut self, t: &txn::Model, kind: TxnKind) {
        self.basis.first_trade.get_or_insert(t.trade_date);
        self.basis.last_trade = Some(t.trade_date);
        self.basis.fees_paid += t.fees + t.taxes;

        match kind {
            TxnKind::Buy | TxnKind::Reinvest | TxnKind::TransferIn => {
                let qty = t.quantity.abs();
                if qty <= DUST {
                    return;
                }
                // Costs of acquisition belong in the basis, not in P&L.
                let total = qty * t.price + t.fees + t.taxes;
                self.push_lot(Lot { qty, cost_per_unit: total / qty, acquired: t.trade_date });
                self.flows.push(CashFlow { date: t.trade_date, amount: -total });
            }

            TxnKind::Sell | TxnKind::TransferOut => {
                let qty = t.quantity.abs();
                if qty <= DUST {
                    return;
                }
                let proceeds = qty * t.price - t.fees - t.taxes;
                let cost = self.consume(qty);
                self.basis.realised_pnl += proceeds - cost;
                self.flows.push(CashFlow { date: t.trade_date, amount: proceeds });
            }

            // Free units: quantity rises, total cost does not, so the average
            // cost per unit falls out of the arithmetic on its own.
            TxnKind::Bonus => {
                let qty = t.quantity.abs();
                if qty > DUST {
                    self.push_lot(Lot { qty, cost_per_unit: 0.0, acquired: t.trade_date });
                }
            }

            // `quantity` is the multiplier: 2.0 means each old share became
            // two. Total cost is untouched; only its spread changes.
            TxnKind::Split => {
                let ratio = t.quantity;
                if ratio > DUST {
                    for lot in self.lots.iter_mut() {
                        lot.qty *= ratio;
                        lot.cost_per_unit /= ratio;
                    }
                }
            }

            TxnKind::Dividend => {
                let cash = if t.net_amount.abs() > DUST {
                    t.net_amount.abs()
                } else {
                    t.quantity.abs() * t.price
                };
                let net = cash - t.fees - t.taxes;
                self.basis.dividends += net;
                self.flows.push(CashFlow { date: t.trade_date, amount: net });
            }
        }
    }

    fn push_lot(&mut self, lot: Lot) {
        match self.method {
            CostBasisMethod::Fifo => self.lots.push_back(lot),
            // Weighted average collapses everything into one blended lot.
            CostBasisMethod::WeightedAverage => {
                let (mut qty, mut cost) = (lot.qty, lot.qty * lot.cost_per_unit);
                if let Some(existing) = self.lots.pop_front() {
                    qty += existing.qty;
                    cost += existing.qty * existing.cost_per_unit;
                }
                let acquired = self.basis.first_trade.unwrap_or(lot.acquired);
                self.lots.push_front(Lot {
                    qty,
                    cost_per_unit: if qty > DUST { cost / qty } else { 0.0 },
                    acquired,
                });
            }
        }
    }

    /// Remove `qty` units from the open lots and return their cost.
    ///
    /// Selling more than is on the books means the opening position was never
    /// imported. Rather than refuse, we release what we have at its real cost
    /// and treat the excess as zero-cost - the position reads as fully sold
    /// and the P&L is flagged-high rather than silently negative.
    fn consume(&mut self, mut qty: f64) -> f64 {
        let mut cost = 0.0;
        while qty > DUST {
            let Some(front) = self.lots.front_mut() else { break };
            let take = qty.min(front.qty);
            cost += take * front.cost_per_unit;
            front.qty -= take;
            qty -= take;
            if front.qty <= DUST {
                self.lots.pop_front();
            }
        }
        cost
    }

    pub fn finish(mut self) -> (CostBasis, Vec<CashFlow>) {
        let qty: f64 = self.lots.iter().map(|l| l.qty).sum();
        let invested: f64 = self.lots.iter().map(|l| l.qty * l.cost_per_unit).sum();
        self.basis.quantity = round_qty(qty);
        self.basis.invested = round_money(invested);
        self.basis.average_cost = if qty > DUST { invested / qty } else { 0.0 };
        self.basis.realised_pnl = round_money(self.basis.realised_pnl);
        self.basis.dividends = round_money(self.basis.dividends);
        (self.basis, self.flows)
    }

    /// Age of the oldest open lot - drives the short/long-term capital-gains
    /// hint in the UI.
    pub fn oldest_open_lot(&self) -> Option<NaiveDate> {
        self.lots.iter().map(|l| l.acquired).min()
    }
}

/// Unrealised P&L against a market price, in the instrument's currency.
pub fn unrealised(basis: &CostBasis, last_price: Option<f64>) -> (Option<f64>, Option<f64>, Option<f64>) {
    let Some(price) = last_price else {
        return (None, None, None);
    };
    let market_value = basis.quantity * price;
    let pnl = market_value - basis.invested;
    (Some(round_money(market_value)), Some(round_money(pnl)), pct_change(basis.invested, market_value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn tx(id: i32, date: NaiveDate, kind: &str, qty: f64, price: f64, fees: f64) -> txn::Model {
        txn::Model {
            id,
            account_id: 1,
            instrument_id: 1,
            trade_date: date,
            kind: kind.to_string(),
            quantity: qty,
            price,
            fees,
            taxes: 0.0,
            net_amount: 0.0,
            currency_code: "INR".into(),
            fx_rate: None,
            external_id: None,
            import_batch_id: None,
            notes: None,
            created_at: chrono::Utc::now().naive_utc(),
        }
    }

    #[test]
    fn fifo_sells_the_oldest_lot_first() {
        let mut book = LotBook::new(CostBasisMethod::Fifo);
        book.apply_all(&[
            tx(1, d(2024, 1, 1), "BUY", 10.0, 100.0, 0.0),
            tx(2, d(2024, 6, 1), "BUY", 10.0, 200.0, 0.0),
            tx(3, d(2025, 1, 1), "SELL", 10.0, 250.0, 0.0),
        ]);
        let (b, _) = book.finish();
        // Sold the 100-rupee lot: 2500 - 1000 = 1500 realised.
        assert_eq!(b.realised_pnl, 1500.0);
        assert_eq!(b.quantity, 10.0);
        assert_eq!(b.invested, 2000.0);
    }

    #[test]
    fn weighted_average_blends_the_cost() {
        let mut book = LotBook::new(CostBasisMethod::WeightedAverage);
        book.apply_all(&[
            tx(1, d(2024, 1, 1), "BUY", 10.0, 100.0, 0.0),
            tx(2, d(2024, 6, 1), "BUY", 10.0, 200.0, 0.0),
            tx(3, d(2025, 1, 1), "SELL", 10.0, 250.0, 0.0),
        ]);
        let (b, _) = book.finish();
        // Average cost 150: 2500 - 1500 = 1000 realised.
        assert_eq!(b.realised_pnl, 1000.0);
        assert_eq!(b.average_cost, 150.0);
    }

    #[test]
    fn fees_land_in_the_cost_basis_not_in_pnl() {
        let mut book = LotBook::new(CostBasisMethod::Fifo);
        book.apply_all(&[tx(1, d(2024, 1, 1), "BUY", 10.0, 100.0, 50.0)]);
        let (b, _) = book.finish();
        assert_eq!(b.invested, 1050.0);
        assert_eq!(b.average_cost, 105.0);
    }

    #[test]
    fn a_split_preserves_total_cost() {
        let mut book = LotBook::new(CostBasisMethod::Fifo);
        book.apply_all(&[
            tx(1, d(2024, 1, 1), "BUY", 10.0, 100.0, 0.0),
            tx(2, d(2024, 6, 1), "SPLIT", 2.0, 0.0, 0.0),
        ]);
        let (b, _) = book.finish();
        assert_eq!(b.quantity, 20.0);
        assert_eq!(b.invested, 1000.0);
        assert_eq!(b.average_cost, 50.0);
    }

    #[test]
    fn bonus_units_dilute_the_average_cost() {
        let mut book = LotBook::new(CostBasisMethod::Fifo);
        book.apply_all(&[
            tx(1, d(2024, 1, 1), "BUY", 10.0, 100.0, 0.0),
            tx(2, d(2024, 6, 1), "BONUS", 10.0, 0.0, 0.0),
        ]);
        let (b, _) = book.finish();
        assert_eq!(b.quantity, 20.0);
        assert_eq!(b.invested, 1000.0);
        assert_eq!(b.average_cost, 50.0);
    }

    #[test]
    fn dividends_are_income_not_a_position_change() {
        let mut book = LotBook::new(CostBasisMethod::Fifo);
        book.apply_all(&[
            tx(1, d(2024, 1, 1), "BUY", 10.0, 100.0, 0.0),
            tx(2, d(2024, 6, 1), "DIVIDEND", 10.0, 5.0, 0.0),
        ]);
        let (b, _) = book.finish();
        assert_eq!(b.quantity, 10.0);
        assert_eq!(b.dividends, 50.0);
        assert_eq!(b.realised_pnl, 0.0);
    }

    #[test]
    fn selling_more_than_held_closes_out_without_panicking() {
        let mut book = LotBook::new(CostBasisMethod::Fifo);
        book.apply_all(&[
            tx(1, d(2024, 1, 1), "BUY", 5.0, 100.0, 0.0),
            tx(2, d(2025, 1, 1), "SELL", 10.0, 120.0, 0.0),
        ]);
        let (b, _) = book.finish();
        assert_eq!(b.quantity, 0.0);
        // 1200 proceeds against only 500 of known cost.
        assert_eq!(b.realised_pnl, 700.0);
    }

    #[test]
    fn a_fully_closed_position_reports_no_average_cost() {
        let mut book = LotBook::new(CostBasisMethod::Fifo);
        book.apply_all(&[
            tx(1, d(2024, 1, 1), "BUY", 10.0, 100.0, 0.0),
            tx(2, d(2025, 1, 1), "SELL", 10.0, 150.0, 0.0),
        ]);
        let (b, _) = book.finish();
        assert_eq!(b.quantity, 0.0);
        assert_eq!(b.invested, 0.0);
        assert_eq!(b.average_cost, 0.0);
    }
}
