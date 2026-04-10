//! Unified product event abstraction and ingestion adapters.
//!
//! This module provides a common interface for receiving products from different sources
//! (QBT TCP and Weather Wire) and normalizing them into a unified event stream.
//!
//! ## Core Types
//!
//! - [`ReceivedProduct`]: Enum representing product receipt events (in-progress, complete, warning)
//! - [`ProductOrigin`]: Source identifier for products (QBT or Weather Wire)
//! - [`IngestEvent`]: Combined stream of products and telemetry events
//!
//! ## Receiver Abstraction
//!
//! - [`IngestConfig`]: Source-specific receiver configuration
//! - [`IngestReceiver`]: Unified receiver lifecycle and event stream API
//!
//! The core crate owns the adaptation from protocol-specific receiver events into
//! a uniform stream of [`IngestEvent`] values.

mod model;
#[cfg(feature = "qbt")]
mod qbt_adapter;
mod receiver;
#[cfg(feature = "wxwire")]
mod wxwire_adapter;

pub use model::{
    IngestError, IngestEvent, IngestTelemetry, IngestWarning, ProductOrigin, ReceivedProduct,
};
pub use receiver::{IngestConfig, IngestReceiver};
