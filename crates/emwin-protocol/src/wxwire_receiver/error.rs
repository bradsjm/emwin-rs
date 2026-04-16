//! Error types for Weather Wire receiver.
//!
//! This module defines the error types used by the Weather Wire XMPP receiver,
//! covering configuration, decoding, lifecycle, and transport errors.

use thiserror::Error;

/// Result alias for weather wire components.
pub type WxWireReceiverResult<T> = Result<T, WxWireReceiverError>;

/// Errors emitted by weather wire decoding and runtime components.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum WxWireReceiverError {
    #[error(transparent)]
    Config(#[from] WxWireConfigError),
    #[error(transparent)]
    Decode(#[from] WxWireDecodeError),
    #[error(transparent)]
    Lifecycle(#[from] WxWireLifecycleError),
    #[error(transparent)]
    Transport(#[from] WxWireTransportError),
}

#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum WxWireConfigError {
    #[error("username must not be empty")]
    EmptyUsername,
    #[error("password must not be empty")]
    EmptyPassword,
    #[error("idle_timeout_secs must be >= 1")]
    ZeroIdleTimeout,
    #[error("event_channel_capacity must be >= 1")]
    ZeroEventChannelCapacity,
    #[error("inbound_channel_capacity must be >= 1")]
    ZeroInboundChannelCapacity,
    #[error("telemetry_emit_interval_secs must be >= 1")]
    ZeroTelemetryEmitInterval,
    #[error("connect_timeout_secs must be >= 1")]
    ZeroConnectTimeout,
    #[error("write_timeout_secs must be >= 1")]
    ZeroWriteTimeout,
}

#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum WxWireDecodeError {
    #[error("invalid xml stanza: {0}")]
    InvalidXml(String),
    #[error("not an xmpp <message/> stanza")]
    NotMessageStanza,
    #[error("missing nwws-oi payload")]
    MissingPayload,
    #[error("weather wire payload is empty")]
    EmptyPayload,
}

#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum WxWireLifecycleError {
    #[error("weather wire client already running")]
    AlreadyRunning,
    #[error("weather wire client not running")]
    NotRunning,
    #[error("weather wire ingress queue full")]
    IngressQueueFull,
    #[error("weather wire ingress queue closed")]
    IngressQueueClosed,
    #[error("weather wire event stream already taken")]
    EventStreamTaken,
    #[error("weather wire client shutdown timeout")]
    ShutdownTimeout,
    #[error("{0}")]
    Internal(String),
}

#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum WxWireTransportError {
    #[error("xmpp connect timeout overflow")]
    ConnectTimeoutOverflow,
    #[error("xmpp connect timeout")]
    ConnectTimeout,
    #[error("failed to connect tcp socket: {0}")]
    TcpConnect(String),
    #[error("invalid room jid: {0}")]
    InvalidRoomJid(String),
    #[error("server does not advertise STARTTLS")]
    MissingStartTls,
    #[error("server did not proceed with STARTTLS")]
    StartTlsRejected,
    #[error("server does not advertise SASL mechanisms")]
    MissingSaslMechanisms,
    #[error("xmpp authentication failed: {0}")]
    AuthenticationFailed(String),
    #[error("server does not advertise resource binding")]
    MissingResourceBinding,
    #[error("unexpected bind response: {0}")]
    UnexpectedBindResponse(String),
    #[error("resource bind failed: {0}")]
    ResourceBindFailed(String),
    #[error("xmpp stream management enable failed: {0}")]
    StreamManagementEnableFailed(String),
    #[error("xmpp join confirmation timeout")]
    JoinConfirmationTimeout,
    #[error("xmpp room join rejected: {0}")]
    JoinRejected(String),
    #[error("xmpp client not connected")]
    ClientNotConnected,
    #[error("xmpp read timeout")]
    ReadTimeout,
    #[error("xmpp read timeout while waiting for {0}")]
    ReadTimeoutWaiting(String),
    #[error("xmpp read failed: {0}")]
    ReadFailed(String),
    #[error("xmpp stream ended")]
    StreamEnded,
    #[error("xmpp write failed: {0}")]
    WriteFailed(String),
    #[error("xmpp write timeout")]
    WriteTimeout,
    #[error("xmpp socket not available")]
    SocketNotAvailable,
    #[error("invalid tls server name")]
    InvalidTlsServerName,
    #[error("tls handshake timeout")]
    TlsHandshakeTimeout,
    #[error("tls handshake failed: {0}")]
    TlsHandshakeFailed(String),
    #[error("{0}")]
    BufferOverflow(String),
    #[error("invalid join presence stanza: {0}")]
    InvalidJoinPresence(String),
}
