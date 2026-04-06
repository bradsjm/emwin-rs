use thiserror::Error;

pub type LiveResult<T> = std::result::Result<T, LiveError>;

#[derive(Debug, Error)]
pub enum LiveError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    AddrParse(#[from] std::net::AddrParseError),
    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),
    #[error(transparent)]
    QbtProtocol(#[from] emwin_protocol::qbt_receiver::QbtProtocolError),
    #[error(transparent)]
    QbtReceiver(#[from] emwin_protocol::qbt_receiver::QbtReceiverError),
    #[error(transparent)]
    WxWireReceiver(#[from] emwin_protocol::wxwire_receiver::WxWireReceiverError),
    #[error(transparent)]
    Ingest(#[from] emwin_protocol::ingest::IngestError),
    #[error(transparent)]
    Persistence(#[from] emwin_db::PersistError),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("runtime failure: {0}")]
    Runtime(String),
}

impl LiveError {
    pub fn invalid_argument(msg: impl Into<String>) -> Self {
        Self::InvalidArgument(msg.into())
    }

    pub fn runtime(msg: impl Into<String>) -> Self {
        Self::Runtime(msg.into())
    }
}
