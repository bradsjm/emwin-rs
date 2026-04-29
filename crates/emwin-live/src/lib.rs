//! Live ingest orchestration and retained-file services.
//!
//! Module ownership is split by responsibility:
//! - `config`: startup options and receiver selection
//! - `ingest`: receiver-driven event orchestration
//! - `file_pipeline`: file completion, enrichment, and persistence planning
//! - `retained`: in-memory retained-file cache and download support
//! - `persistence`: storage target configuration and persistence wiring
//! - `events` and `types`: service-facing event and option shaping
//! - `runtime`: public runtime entrypoint

#![deny(missing_docs)]

mod archive_postprocess;
mod config;
mod default_servers;
mod error;
mod events;
mod file_pipeline;
mod ingest;
mod persistence;
mod product_processor;
mod retained;
mod runtime;
mod shared;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
mod types;

pub use emwin_service::{
    FileEventFilter, FileFilterInput, FileFilterInputError, IncidentBroadcastEvent,
    LiveBroadcastEvent, LiveEventKind, LiveStatsSnapshot, LiveTelemetry, RetainedFile, SourceKind,
};
pub use error::{LiveError, LiveResult};
pub use runtime::LiveRuntime;
pub use types::{LiveOptions, ReceiverKind};
