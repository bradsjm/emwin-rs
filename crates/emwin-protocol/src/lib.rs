//! Core receivers, protocol code, and ingest adapters for EMWIN feeds.
//!
//! The crate exposes two transport-specific receivers, `qbt_receiver` and `wxwire_receiver`, plus
//! the `ingest` layer that normalizes their output into one event stream. Most users should start
//! at `ingest` unless they need transport-specific controls.
//!
//! # Features
//!
//! - **qbt**: EMWIN QBT satellite feed receiver
//! - **wxwire**: Weather Wire/XMPP receiver
//! - **telemetry-serde**: Enable serde serialization for telemetry types
//!
//! # Example
//!
//! Connect to EMWIN QBT feed and process incoming products:
//!
//! ```no_run
//! use emwin_protocol::ingest::{IngestConfig, IngestReceiver};
//! use emwin_protocol::qbt_receiver::{QbtDecodeConfig, QbtReceiverConfig, default_qbt_upstream_servers};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut receiver = IngestReceiver::build(IngestConfig::Qbt(QbtReceiverConfig {
//!         email: "you@example.com".to_string(),
//!         servers: default_qbt_upstream_servers(),
//!         server_list_path: None,
//!         follow_server_list_updates: true,
//!         reconnect_delay_secs: 5,
//!         connection_timeout_secs: 5,
//!         watchdog_timeout_secs: 49,
//!         max_exceptions: 10,
//!         decode: QbtDecodeConfig::default(),
//!     }))?;
//!     receiver.start()?;
//!     receiver.stop().await?;
//!     Ok(())
//! }
//! ```

#[cfg(any(feature = "qbt", feature = "wxwire"))]
pub mod ingest;
#[cfg(feature = "qbt")]
pub mod qbt_receiver;
#[cfg(any(feature = "qbt", feature = "wxwire"))]
mod runtime_support;
#[cfg(feature = "wxwire")]
pub mod wxwire_receiver;

/// Unstable API surface. Items in this module may change without stability guarantees.
pub mod unstable {
    #[cfg(feature = "qbt")]
    pub mod qbt_receiver {
        pub use crate::qbt_receiver::unstable::*;
    }

    #[cfg(feature = "wxwire")]
    pub mod wxwire_receiver {
        pub use crate::wxwire_receiver::unstable::*;
    }
}
