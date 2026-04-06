//! Command implementations for the EMWIN CLI.
//!
//! This module contains shared CLI presentation helpers.
//!
//! ## Output Contract
//!
//! Live command diagnostics are written to `stderr` via `tracing`.

pub mod query;
pub mod query_output;
