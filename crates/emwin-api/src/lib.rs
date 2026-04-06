//! Reusable HTTP/SSE server surface for EMWIN live ingest.
//!
//! This crate owns the server runtime, API filter grammar, and the HTTP/OpenAPI boundary used by
//! `emwin-cli server`.

mod cmd;
pub mod error;
mod live;

pub use error::{ApiError, ApiResult};
pub use live::server::{HttpServerOptions, serve};
