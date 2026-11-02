//! Shared vocabulary types.
//!
//! Every enum here is persisted as plain TEXT rather than a native DB enum.
//! That is deliberate: SQLite has no enum type and Postgres enums are painful
//! to evolve, so TEXT keeps one schema working on both backends.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Generates an enum that round-trips through a lowercase-ish string form.
macro_rules! str_enum {
    ($(#[$outer:meta])* $name:ident { $( $(#[$meta:meta])* $variant:ident => $text:literal ),+ $(,)? }) => {
        $(#[$outer])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
        pub enum $name {
            $( $(#[$meta])* #[serde(rename = $text)] $variant ),+
        }

        impl $name {
            pub fn as_str(&self) -> &'static str {
                match self { $( $name::$variant => $text ),+ }
            }
            pub const ALL: &'static [$name] = &[ $( $name::$variant ),+ ];
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = String;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let k = s.trim().to_ascii_uppercase().replace([' ', '-'], "_");
                match k.as_str() {
                    $( $text => Ok($name::$variant), )+
                    _ => Err(format!("unknown {}: '{}'", stringify!($name), s)),
                }
            }
        }
    };
}

str_enum!(AssetKind {
    Equity      => "EQUITY",
    Etf         => "ETF",
    MutualFund  => "MUTUAL_FUND",
    Bond        => "BOND",
    Reit        => "REIT",
    Commodity   => "COMMODITY",
    Cash        => "CASH",
});

str_enum!(AccountKind {
    /// Indian demat / trading account (stocks).
    DematIn     => "DEMAT_IN",
    /// Indian mutual-fund folio (AMC or platform).
    MfFolioIn   => "MF_FOLIO_IN",
    /// Any non-Indian brokerage (IBKR, Stake, CommSec, Tiger, ...).
    BrokerGlobal=> "BROKER_GLOBAL",
    /// Savings / current / transaction account.
    Bank        => "BANK",
    /// Container for term deposits held at an institution.
    Deposit     => "DEPOSIT",
});

str_enum!(TxnKind {
    Buy         => "BUY",
    Sell        => "SELL",
    Dividend    => "DIVIDEND",
    /// Mutual-fund dividend reinvested into additional units.
    Reinvest    => "REINVEST",
    /// Free units/shares (bonus issue). Quantity up, cost unchanged.
    Bonus       => "BONUS",
    /// Stock split. `quantity` carries the ratio multiplier (2.0 = 1:2 split).
    Split       => "SPLIT",
    /// Units/shares moved in from elsewhere, carrying their original cost.
    TransferIn  => "TRANSFER_IN",
    TransferOut => "TRANSFER_OUT",
});

str_enum!(DepositKind {
    /// Lump-sum term deposit (Indian FD, SG fixed deposit, AU term deposit).
    FixedDeposit     => "FIXED_DEPOSIT",
    /// Indian recurring deposit - fixed instalment every period.
    RecurringDeposit => "RECURRING_DEPOSIT",
    /// Employer/government schemes with a maturity (PPF, NSC, ...).
    Scheme           => "SCHEME",
});

str_enum!(Compounding {
    Simple      => "SIMPLE",
    Monthly     => "MONTHLY",
    Quarterly   => "QUARTERLY",
    HalfYearly  => "HALF_YEARLY",
    Annual      => "ANNUAL",
    AtMaturity  => "AT_MATURITY",
});

impl Compounding {
    /// Compounding periods per year. `Simple` and `AtMaturity` are handled
    /// separately by the accrual maths and return 0 / 1 respectively.
    pub fn periods_per_year(&self) -> f64 {
        match self {
            Compounding::Simple => 0.0,
            Compounding::Monthly => 12.0,
            Compounding::Quarterly => 4.0,
            Compounding::HalfYearly => 2.0,
            Compounding::Annual => 1.0,
            Compounding::AtMaturity => 1.0,
        }
    }
}

str_enum!(PayoutFrequency {
    /// Interest rolls up and is paid with the principal.
    Cumulative  => "CUMULATIVE",
    Monthly     => "MONTHLY",
    Quarterly   => "QUARTERLY",
    HalfYearly  => "HALF_YEARLY",
    Annual      => "ANNUAL",
});

str_enum!(DepositStatus {
    Active      => "ACTIVE",
    Matured     => "MATURED",
    Closed      => "CLOSED",
    /// Broken before maturity (premature withdrawal).
    Broken      => "BROKEN",
});

str_enum!(MarginMode {
    /// Use the provider rate verbatim.
    None        => "NONE",
    /// Shift the rate by a percentage, e.g. -0.5 knocks 0.5% off.
    Percent     => "PERCENT",
    /// Shift the rate by an absolute amount in quote-currency units.
    Absolute    => "ABSOLUTE",
    /// Take off what Wise charges to convert, which depends on the amount.
    Wise        => "WISE",
});

str_enum!(ImportKind {
    IndiaEquity => "INDIA_EQUITY",
    IndiaMf     => "INDIA_MF",
    Global      => "GLOBAL",
    Deposits    => "DEPOSITS",
    CashBalances=> "CASH_BALANCES",
    Instruments => "INSTRUMENTS",
});

str_enum!(ImportStatus {
    Pending     => "PENDING",
    Completed   => "COMPLETED",
    /// Some rows landed, some were rejected.
    Partial     => "PARTIAL",
    Failed      => "FAILED",
});

str_enum!(
    /// How a holding's cost basis is computed.
    CostBasisMethod {
        /// First in, first out - the Indian statutory method for capital gains.
        Fifo            => "FIFO",
        WeightedAverage => "WEIGHTED_AVERAGE",
    }
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_case_and_separator_insensitively() {
        assert_eq!(TxnKind::from_str("buy").unwrap(), TxnKind::Buy);
        assert_eq!(TxnKind::from_str(" Transfer-In ").unwrap(), TxnKind::TransferIn);
        assert_eq!(AssetKind::from_str("mutual fund").unwrap(), AssetKind::MutualFund);
        assert!(TxnKind::from_str("nonsense").is_err());
    }

    #[test]
    fn round_trips_through_text() {
        for k in AccountKind::ALL {
            assert_eq!(AccountKind::from_str(k.as_str()).unwrap(), *k);
        }
    }
}
