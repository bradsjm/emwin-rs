//! Primary live command for running the EMWIN HTTP server with SSE endpoints.
//!
//! This module provides an HTTP server that:
//! - Streams events via Server-Sent Events (SSE)
//! - Serves completed files for download
//! - Provides health and metrics endpoints
//! - Supports CORS for browser clients

mod cors;
mod events;
mod openapi;
mod runtime;
mod server_http;
pub(crate) mod types;

pub use runtime::serve;
pub use types::{ApiArchiveStatus, ApiServices, HttpServerOptions};

pub(crate) use cors::build_cors_layer;
pub(crate) use events::{publish, publish_incident_change};
#[cfg(test)]
pub(crate) use test_support::build_router;

fn log_info(quiet: bool, msg: &str) {
    if !quiet {
        tracing::info!("{msg}");
    }
}

#[cfg(test)]
mod test_support {
    use super::server_http;
    use super::types::AppState;
    use crate::error::ApiResult;
    use axum::Router;
    use std::sync::Arc;

    pub(crate) fn build_router(
        state: Arc<AppState>,
        cors_origin: Option<String>,
    ) -> ApiResult<Router> {
        let cors = super::build_cors_layer(cors_origin)?;
        Ok(server_http::build_router(state, cors))
    }
}
