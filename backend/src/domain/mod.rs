//! Pure financial logic - no database, no HTTP, no network.
//!
//! Everything here takes plain values and returns plain values, which is why
//! it is the part of the codebase with real test coverage.

pub mod deposits;
pub mod fx;
pub mod holdings;
pub mod money;
pub mod retirement;
pub mod scheme;
pub mod xirr;
