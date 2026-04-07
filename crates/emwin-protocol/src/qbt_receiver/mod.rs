//! QBT receiver runtime and protocol support.
//!
//! This module groups the streaming decoder, connection-management logic, file assembly, and relay
//! support needed for the EMWIN QBT feed. The public API exposes stable runtime types while
//! keeping lower-level protocol helpers available through curated re-exports.
//!
//! Module ownership is split by responsibility:
//! - `client`: upstream connection lifecycle, reconnects, watchdogs, and event fan-out
//! - `config`: runtime bounds and decoder policy configuration
//! - `file`: completed-file and segment assembly
//! - `protocol`: wire framing, auth, checksums, compression, and server-list parsing
//! - `relay`: downstream relay runtime and health reporting
//! - `stream`: raw file and segment stream adapters
//! - `error`: typed runtime and protocol errors

mod client;
mod config;
mod error;
mod file;
mod protocol;
mod relay;
mod stream;

pub use client::{
    QbtReceiver, QbtReceiverBuilder, QbtReceiverClient, QbtReceiverEvent,
    QbtReceiverTelemetrySnapshot,
};
pub use config::{DEFAULT_QBT_UPSTREAM_SERVERS, default_qbt_upstream_servers};
pub use config::{QbtChecksumPolicy, QbtDecodeConfig, QbtReceiverConfig, QbtV2CompressionPolicy};
pub use error::{QbtProtocolError, QbtReceiverConfigError, QbtReceiverError, QbtReceiverResult};
pub use file::{QbtCompletedFile, QbtFileAssembler, QbtSegmentAssembler};
pub use protocol::auth::{
    LOGON_PREFIX, LOGON_SUFFIX, REAUTH_INTERVAL_SECS, build_logon_message, parse_logon_message,
    xor_ff,
};
pub use protocol::checksum::calculate_qbt_checksum;
pub use protocol::codec::{QbtFrameDecoder, QbtFrameEncoder, QbtProtocolDecoder};
pub use protocol::model::{
    QbtAuthMessage, QbtFrameEvent, QbtProtocolVersion, QbtProtocolWarning, QbtSegment,
    QbtServerList,
};
pub use protocol::server_list::parse_qbt_server;
pub use protocol::server_list_wire::{QbtServerListWireScanner, build_server_list_wire};
pub use relay::{
    QbtRelayConfig, QbtRelayError, QbtRelayHealthSnapshot, QbtRelayMetricsSnapshot, QbtRelayResult,
    QbtRelayState, run_qbt_relay,
};

pub mod unstable {
    pub use super::client::reconnect::{EndpointRotator, next_backoff_secs};
    pub use super::client::watchdog::{HealthObserver, Watchdog};
    pub use super::protocol::auth::{build_logon_message, xor_ff};
    pub use super::protocol::server_list::{parse_server_list_frame, parse_simple_server_list};
    pub use super::stream::file_stream::QbtFileStream;
    pub use super::stream::segment_stream::QbtSegmentStream;
}
