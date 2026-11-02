//! Spreadsheet import: read, validate, apply.
//!
//! `specs` defines the accepted format once and everything else - templates,
//! validation, documentation - is derived from it.

pub mod apply;
pub mod portfolio;
pub mod reader;
pub mod specs;

pub use apply::{import, ImportOutcome, RowError};
pub use specs::{all_specs, spec_for, ImportSpec};
