//! HTTP/SSE server surface for EMWIN live ingest.
//!
//! This crate owns the HTTP/OpenAPI boundary used by `emwin-cli server`.

#![deny(missing_docs)]
#![recursion_limit = "4096"]

pub mod error;
mod server;
mod server_support;

pub use error::{ApiError, ApiResult};
pub use server::{ApiArchiveStatus, ApiServices, HttpServerOptions, serve};
