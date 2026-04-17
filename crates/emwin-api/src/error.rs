//! Error types for the EMWIN API crate.

use thiserror::Error;

/// Result type for API operations.
pub type ApiResult<T> = std::result::Result<T, ApiError>;

/// Errors produced by the HTTP/OpenAPI server layer.
#[derive(Debug, Error)]
pub enum ApiError {
    /// File or socket I/O failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// JSON serialization or parsing failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Socket address parsing failed.
    #[error(transparent)]
    AddrParse(#[from] std::net::AddrParseError),
    /// A background task failed to join.
    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),
    /// Live runtime interaction failed.
    #[error(transparent)]
    Live(#[from] emwin_live::LiveError),
    /// Service-layer interaction failed.
    #[error(transparent)]
    Service(#[from] emwin_service::ServiceError),
    /// The caller supplied invalid arguments.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    /// The server encountered an unrecoverable failure.
    #[error("runtime failure: {0}")]
    Runtime(String),
}

impl ApiError {
    /// Builds an invalid-argument error.
    pub fn invalid_argument(msg: impl Into<String>) -> Self {
        Self::InvalidArgument(msg.into())
    }

    /// Builds a runtime-failure error.
    pub fn runtime(msg: impl Into<String>) -> Self {
        Self::Runtime(msg.into())
    }
}
