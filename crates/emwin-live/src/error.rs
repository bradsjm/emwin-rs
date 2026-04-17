//! Error types for the live ingest runtime.

use thiserror::Error;

/// Result type for live runtime operations.
pub type LiveResult<T> = std::result::Result<T, LiveError>;

/// Errors produced by the live ingest runtime.
#[derive(Debug, Error)]
pub enum LiveError {
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
    /// QBT protocol handling failed.
    #[error(transparent)]
    QbtProtocol(#[from] emwin_protocol::qbt_receiver::QbtProtocolError),
    /// QBT receiver runtime failed.
    #[error(transparent)]
    QbtReceiver(#[from] emwin_protocol::qbt_receiver::QbtReceiverError),
    /// Weather Wire receiver runtime failed.
    #[error(transparent)]
    WxWireReceiver(#[from] emwin_protocol::wxwire_receiver::WxWireReceiverError),
    /// Ingest orchestration failed.
    #[error(transparent)]
    Ingest(#[from] emwin_protocol::ingest::IngestError),
    /// Persistence runtime failed.
    #[error(transparent)]
    Persistence(#[from] emwin_db::PersistError),
    /// The caller supplied invalid arguments.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    /// The runtime encountered an unrecoverable failure.
    #[error("runtime failure: {0}")]
    Runtime(String),
}

impl LiveError {
    /// Builds an invalid-argument error.
    pub fn invalid_argument(msg: impl Into<String>) -> Self {
        Self::InvalidArgument(msg.into())
    }

    /// Builds a runtime-failure error.
    pub fn runtime(msg: impl Into<String>) -> Self {
        Self::Runtime(msg.into())
    }
}
