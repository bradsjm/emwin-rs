use thiserror::Error;

pub type ServiceResult<T> = Result<T, ServiceError>;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("{0}")]
    InvalidRequest(String),
    #[error("{0}")]
    InvalidConfig(String),
    #[error("{0}")]
    NotConfigured(String),
    #[error("{0}")]
    Runtime(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl ServiceError {
    pub fn invalid_argument(msg: impl Into<String>) -> Self {
        Self::InvalidRequest(msg.into())
    }

    pub fn runtime(msg: impl Into<String>) -> Self {
        Self::Runtime(msg.into())
    }
}
