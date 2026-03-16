use thiserror::Error;

use std::io::ErrorKind;

/// Result type used by persistence runtime operations.
pub type PersistResult<T> = std::result::Result<T, PersistError>;

/// Errors produced by the async persistence runtime and blob writers.
#[derive(Debug, Error)]
pub enum PersistError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("s3 {operation} failed: {message}")]
    S3 {
        operation: &'static str,
        retryable: bool,
        message: String,
    },
    #[error("persistence runtime is closed")]
    Closed,
    #[error("invalid persistence config: {0}")]
    InvalidConfig(String),
    #[error("invalid persistence request: {0}")]
    InvalidRequest(String),
}

impl PersistError {
    pub fn s3_client(operation: &'static str, err: &s3::error::S3Error) -> Self {
        let status = s3_status_code(err);
        let retryable = is_retryable_s3_error(err, status);
        let message = match status {
            Some(status) => format!("HTTP {status}: {err}"),
            None => err.to_string(),
        };

        Self::S3 {
            operation,
            retryable,
            message,
        }
    }

    pub fn s3_response(operation: &'static str, status: u16, message: impl Into<String>) -> Self {
        Self::S3 {
            operation,
            retryable: is_retryable_s3_status(status),
            message: format!("HTTP {status}: {}", message.into()),
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
            Self::S3 { retryable, .. } => *retryable,
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
            Self::S3 { .. } => "s3_unavailable",
            Self::Join(_) => "runtime_join_failure",
            Self::Json(_) => "json_failure",
            Self::Migration(_) => "database_migration_failure",
            Self::Closed => "runtime_closed",
            Self::InvalidConfig(_) => "invalid_config",
            Self::InvalidRequest(_) => "invalid_request",
        }
    }
}

fn s3_status_code(err: &s3::error::S3Error) -> Option<u16> {
    match err {
        s3::error::S3Error::HttpFailWithBody(status, _) => Some(*status),
        _ => None,
    }
}

fn is_retryable_s3_error(err: &s3::error::S3Error, status: Option<u16>) -> bool {
    if status.is_some_and(is_retryable_s3_status) {
        return true;
    }

    match err {
        s3::error::S3Error::Io(_) => true,
        s3::error::S3Error::Reqwest(_) | s3::error::S3Error::ReqwestHeaderToStr(_) => true,
        s3::error::S3Error::Http(_) => false,
        s3::error::S3Error::HttpFail => false,
        s3::error::S3Error::HttpFailWithBody(_, _) => false,
        s3::error::S3Error::Credentials(_)
        | s3::error::S3Error::Region(_)
        | s3::error::S3Error::UrlParse(_)
        | s3::error::S3Error::InvalidHeaderValue(_)
        | s3::error::S3Error::InvalidHeaderName(_)
        | s3::error::S3Error::WLCredentials
        | s3::error::S3Error::RLCredentials
        | s3::error::S3Error::CredentialsReadLock
        | s3::error::S3Error::CredentialsWriteLock => false,
        _ => false,
    }
}

fn is_retryable_s3_status(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504)
}

#[cfg(test)]
mod tests {
    use super::PersistError;

    #[test]
    fn s3_retryability_marks_retryable_status_codes() {
        for status in [408, 429, 500, 502, 503, 504] {
            let err = PersistError::s3_client(
                "put_object",
                &s3::error::S3Error::HttpFailWithBody(status, "transient".to_string()),
            );
            assert!(err.is_retryable(), "status {status} should retry");
        }
    }

    #[test]
    fn s3_retryability_rejects_non_transient_status_codes() {
        for status in [400, 401, 403, 404] {
            let err = PersistError::s3_client(
                "put_object",
                &s3::error::S3Error::HttpFailWithBody(status, "permanent".to_string()),
            );
            assert!(!err.is_retryable(), "status {status} should not retry");
        }
    }

    #[test]
    fn s3_retryability_treats_transport_errors_as_retryable() {
        let err = PersistError::s3_client(
            "put_object",
            &s3::error::S3Error::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "network timeout",
            )),
        );
        assert!(err.is_retryable());
        assert_eq!(err.failure_class(), "s3_unavailable");
    }

    #[test]
    fn s3_response_marks_retryable_status_codes() {
        let err = PersistError::s3_response("create_bucket", 503, "service unavailable");
        assert!(err.is_retryable());
    }

    #[test]
    fn s3_response_rejects_non_retryable_status_codes() {
        let err = PersistError::s3_response("create_bucket", 403, "forbidden");
        assert!(!err.is_retryable());
    }
}
