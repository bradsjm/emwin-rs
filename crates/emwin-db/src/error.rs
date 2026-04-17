use thiserror::Error;

use std::io::ErrorKind;

/// Result type used by persistence runtime operations.
pub type PersistResult<T> = std::result::Result<T, PersistError>;

/// Errors produced by the async persistence runtime and blob writers.
#[derive(Debug, Error)]
pub enum PersistError {
    /// File or socket I/O failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A background task failed to join.
    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),
    /// JSON serialization or parsing failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// SQLx query or connection operation failed.
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    /// Database migration application failed.
    #[error(transparent)]
    Migration(#[from] sqlx::migrate::MigrateError),
    /// An object-store backend operation failed.
    #[error("object store {operation} failed: {message}")]
    ObjectStore {
        /// Backend operation name.
        operation: &'static str,
        /// Whether the failure is retryable.
        retryable: bool,
        /// Human-readable backend error message.
        message: String,
    },
    /// The persistence runtime has already been closed.
    #[error("persistence runtime is closed")]
    Closed,
    /// The caller supplied an invalid configuration.
    #[error("invalid persistence config: {0}")]
    InvalidConfig(String),
    /// The caller supplied an invalid request.
    #[error("invalid persistence request: {0}")]
    InvalidRequest(String),
}

impl PersistError {
    /// Builds an object-store failure with retry classification.
    pub fn object_store(
        operation: &'static str,
        retryable: bool,
        message: impl Into<String>,
    ) -> Self {
        Self::ObjectStore {
            operation,
            retryable,
            message: message.into(),
        }
    }

    /// Returns true when the operation should be retried after a backoff delay.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Io(err) => matches!(
                err.kind(),
                ErrorKind::ConnectionRefused
                    | ErrorKind::ConnectionReset
                    | ErrorKind::ConnectionAborted
                    | ErrorKind::NotConnected
                    | ErrorKind::BrokenPipe
                    | ErrorKind::TimedOut
                    | ErrorKind::Interrupted
                    | ErrorKind::WriteZero
                    | ErrorKind::UnexpectedEof
                    | ErrorKind::StorageFull
            ),
            Self::Sqlx(err) => matches!(
                err,
                sqlx::Error::Io(_)
                    | sqlx::Error::Tls(_)
                    | sqlx::Error::PoolTimedOut
                    | sqlx::Error::PoolClosed
                    | sqlx::Error::WorkerCrashed
            ),
            Self::ObjectStore { retryable, .. } => *retryable,
            Self::Join(_)
            | Self::Json(_)
            | Self::Migration(_)
            | Self::Closed
            | Self::InvalidConfig(_)
            | Self::InvalidRequest(_) => false,
        }
    }

    /// Returns true when a Postgres pool should be discarded before the next attempt.
    pub fn should_reset_postgres_pool(&self) -> bool {
        matches!(
            self,
            Self::Sqlx(
                sqlx::Error::Io(_)
                    | sqlx::Error::Tls(_)
                    | sqlx::Error::PoolTimedOut
                    | sqlx::Error::PoolClosed
                    | sqlx::Error::WorkerCrashed
            )
        )
    }

    /// Returns a stable failure class for log throttling.
    pub fn failure_class(&self) -> &'static str {
        match self {
            Self::Io(err) if err.kind() == ErrorKind::StorageFull => "storage_full",
            Self::Io(_) => "filesystem_unavailable",
            Self::Sqlx(_) => "database_unavailable",
            Self::ObjectStore { .. } => "object_store_unavailable",
            Self::Join(_) => "runtime_join_failure",
            Self::Json(_) => "json_failure",
            Self::Migration(_) => "database_migration_failure",
            Self::Closed => "runtime_closed",
            Self::InvalidConfig(_) => "invalid_config",
            Self::InvalidRequest(_) => "invalid_request",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PersistError;

    #[test]
    fn object_store_retryability_can_be_retryable() {
        let err = PersistError::object_store("put_object", true, "network timeout");
        assert!(err.is_retryable());
        assert_eq!(err.failure_class(), "object_store_unavailable");
    }

    #[test]
    fn object_store_retryability_can_be_non_retryable() {
        let err = PersistError::object_store("put_object", false, "permission denied");
        assert!(!err.is_retryable());
    }
}
