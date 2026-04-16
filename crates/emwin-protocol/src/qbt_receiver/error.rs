//! Error types for emwin-protocol QBT receiver.

use thiserror::Error;

/// Result type alias using [`QbtReceiverError`] as the error type.
pub type QbtReceiverResult<T> = Result<T, QbtReceiverError>;

/// Primary error type for QBT receiver components.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum QbtReceiverError {
    /// Configuration validation failed.
    #[error("invalid config: {0}")]
    Config(#[from] QbtReceiverConfigError),
    /// Protocol parsing or validation error.
    #[error("protocol error: {0}")]
    Protocol(#[from] QbtProtocolError),
    /// Underlying I/O operation failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Outbound socket write exceeded the configured timeout.
    #[error("socket write timeout")]
    WriteTimeout,
    /// Client shutdown exceeded the allowed timeout and was aborted.
    #[error("client shutdown timeout")]
    ShutdownTimeout,
    /// Client lifecycle operation failed (start/stop).
    #[error("client lifecycle error: {0}")]
    Lifecycle(String),
}

/// Errors related to receiver configuration validation.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum QbtReceiverConfigError {
    /// Email address is empty or whitespace-only.
    #[error("email must not be empty")]
    EmptyEmail,
    /// No servers were configured.
    #[error("at least one server is required")]
    NoServers,
    /// Write timeout must be at least one second.
    #[error("write_timeout_secs must be >= 1")]
    ZeroWriteTimeout,
}

/// Errors related to protocol parsing and validation.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum QbtProtocolError {
    /// Frame header does not match expected format.
    #[error("invalid frame header")]
    InvalidHeader,
    /// Body length is out of valid range.
    #[error("invalid body length: {0}")]
    InvalidBodyLength(usize),
    /// Frame type is not supported.
    #[error("unsupported frame")]
    UnsupportedFrame,
    /// Required field is missing from the frame.
    #[error("missing field: {0}")]
    MissingField(&'static str),
    /// Zlib decompression failed.
    #[error("decompression failed: {0}")]
    Decompression(String),
    /// Checksum validation failed.
    #[error("checksum mismatch")]
    ChecksumMismatch,
    /// Frame type is invalid for current state.
    #[error("invalid frame type")]
    InvalidFrameType,
    /// Frame contains invalid UTF-8 sequences.
    #[error("invalid utf8 in frame: {0}")]
    InvalidUtf8(String),
    /// UTF-8 decoding failed.
    #[error("utf8 decode failed: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}
