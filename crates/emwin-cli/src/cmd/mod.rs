//! Command implementations for the EMWIN CLI.
//!
//! This module contains shared CLI presentation helpers.
//!
//! ## Output Contract
//!
//! Live command diagnostics are written to `stderr` via `tracing`.
//!
//! Shared event formatting lives here. The live server runtime is implemented directly under
//! `crate::live`.

pub mod event_output;
pub mod query;
pub mod query_output;
