//! Alert worker runtime and outbound delivery for `emwin-rs`.

#![deny(missing_docs)]

mod error;
mod worker;

pub use error::{AlertError, AlertResult};
pub use worker::{
    AlertDispatchConfig, AlertDispatchOutcome, AlertWorkerConfig, AlertWorkerStats,
    AlertWorkerStatsSnapshot, TestAlertNotification, run_worker, send_test_notification,
};
