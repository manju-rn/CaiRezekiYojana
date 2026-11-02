//! Application services - the layer between HTTP handlers and the database.
//!
//! Handlers here stay thin: parse, delegate, serialise. Anything worth
//! testing lives either in `crate::domain` (pure) or in one of these modules.

pub mod audit;
pub mod cash;
pub mod deposits;
pub mod dhan_yatra;
pub mod lookthrough;
pub mod networth;
pub mod plan_sessions;
pub mod performance;
pub mod plan_shares;
pub mod portfolio_import;
pub mod planner;
pub mod portfolio;
pub mod prefs;
pub mod purge;
pub mod refresh;
pub mod retirement;
pub mod text_size;
pub mod users;
pub mod visibility;
