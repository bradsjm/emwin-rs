#![allow(missing_docs)]

use thiserror::Error;

pub type AlertResult<T> = Result<T, AlertError>;

#[derive(Debug, Error)]
pub enum AlertError {
    #[error(transparent)]
    Db(#[from] emwin_db::PersistError),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    HttpMiddleware(#[from] reqwest_middleware::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Template(#[from] minijinja::Error),
    #[error("invalid alert worker configuration: {0}")]
    InvalidConfig(String),
}
