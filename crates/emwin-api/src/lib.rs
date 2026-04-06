//! Reusable HTTP/SSE server surface for EMWIN live ingest.
//!
//! This crate owns the server runtime, API filter grammar, and the HTTP/OpenAPI boundary used by
//! `emwin-cli server`.

pub mod archive_filter;
pub mod cmd;
mod default_servers;
pub mod error;
mod live;

pub use error::{ApiError, ApiResult};
pub use live::server::{ServerOptions, run};

/// Supported upstream receiver backends for server mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverKind {
    /// QBT/EMWIN TCP receiver.
    Qbt,
    /// Weather Wire XMPP receiver.
    Wxwire,
}
