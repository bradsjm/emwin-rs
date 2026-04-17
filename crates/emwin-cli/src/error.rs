//! Error types for the EMWIN CLI.
//!
//! This module defines the error types used throughout the CLI application,
//! providing a unified error handling interface that wraps errors from
//! underlying libraries (emwin-protocol) and CLI-specific errors.

use thiserror::Error;

/// Result type alias for CLI operations.
pub type CliResult<T> = std::result::Result<T, CliError>;

#[derive(Debug, Error)]
pub enum CliError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    AddrParse(#[from] std::net::AddrParseError),
    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),
    #[cfg(feature = "relay")]
    #[error(transparent)]
    QbtProtocol(#[from] emwin_protocol::qbt_receiver::QbtProtocolError),
    #[cfg(feature = "relay")]
    #[error(transparent)]
    QbtReceiver(#[from] emwin_protocol::qbt_receiver::QbtReceiverError),
    #[cfg(any(feature = "query", feature = "server", feature = "alert-worker"))]
    #[error(transparent)]
    Persistence(#[from] emwin_db::PersistError),
    #[cfg(feature = "query")]
    #[error(transparent)]
    Service(#[from] emwin_service::ServiceError),
    #[cfg(feature = "server")]
    #[error(transparent)]
    Live(#[from] emwin_live::LiveError),
    #[cfg(feature = "server")]
    #[error(transparent)]
    Api(#[from] emwin_api::ApiError),
    #[cfg(any(feature = "query", feature = "relay"))]
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[cfg(any(feature = "query", feature = "alert-worker", feature = "relay"))]
    #[error("runtime failure: {0}")]
    Runtime(String),
}

impl CliError {
    #[cfg(any(feature = "query", feature = "relay"))]
    pub fn invalid_argument(msg: impl Into<String>) -> Self {
        Self::InvalidArgument(msg.into())
    }

    #[cfg(any(feature = "query", feature = "relay"))]
    pub fn runtime(msg: impl Into<String>) -> Self {
        Self::Runtime(msg.into())
    }
}
