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

mod archive_postprocess;
mod config;
mod default_servers;
mod error;
mod events;
mod file_pipeline;
pub mod filter;
mod ingest;
mod persistence;
mod retained;
mod runtime;
mod shared;
mod types;

pub use emwin_service::{
    IncidentBroadcastEvent, LiveBroadcastEvent, LiveEventKind, LiveStatsSnapshot, LiveTelemetry,
    RetainedFile, SourceKind,
};
pub use error::{LiveError, LiveResult};
pub use filter::{FileEventFilter, FileFilterInput, FileFilterInputError};
pub use runtime::LiveRuntime;
pub use types::{LiveOptions, ReceiverKind};
