//! Configuration types for emwin-protocol.
//!
//! This module defines configuration structures for the protocol decoder
//! and the client runtime, along with policy enums for checksum validation
//! and compression handling.

#![allow(missing_docs)]

use std::path::PathBuf;

pub const DEFAULT_QBT_UPSTREAM_SERVERS: [(&str, u16); 4] = [
    ("emwin.weathermessage.com", 2211),
    ("master.weathermessage.com", 2211),
    ("emwin.interweather.net", 1000),
    ("wxmesg.upstateweather.com", 2211),
];

pub fn default_qbt_upstream_servers() -> Vec<(String, u16)> {
    DEFAULT_QBT_UPSTREAM_SERVERS
        .iter()
        .map(|(host, port)| ((*host).to_string(), *port))
        .collect()
}

/// Policy for handling checksum validation failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QbtChecksumPolicy {
    /// Drop segments with invalid checksums and emit a warning.
    StrictDrop,
}

/// Policy for handling V2 protocol compression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QbtV2CompressionPolicy {
    /// Only attempt decompression if the data has a valid zlib header.
    RequireZlibHeader,
    /// Always attempt decompression regardless of header.
    TryAlways,
}

/// Configuration for the protocol decoder.
#[derive(Debug, Clone)]
pub struct QbtDecodeConfig {
    /// Checksum validation policy.
    pub checksum_policy: QbtChecksumPolicy,
    /// Compression handling policy for V2 frames.
    pub compression_policy: QbtV2CompressionPolicy,
    /// Maximum allowed body size for V2 frames (in bytes).
    pub max_v2_body_size: usize,
    /// Maximum buffered bytes allowed while waiting for a server-list terminator.
    pub max_server_list_frame_bytes: usize,
}

impl Default for QbtDecodeConfig {
    fn default() -> Self {
        Self {
            checksum_policy: QbtChecksumPolicy::StrictDrop,
            compression_policy: QbtV2CompressionPolicy::RequireZlibHeader,
            max_v2_body_size: 1024,
            max_server_list_frame_bytes: 65_536,
        }
    }
}

/// Configuration for the EMWIN client.
#[derive(Debug, Clone)]
pub struct QbtReceiverConfig {
    /// User email address for authentication.
    pub email: String,
    /// List of server endpoints as (host, port) tuples.
    pub servers: Vec<(String, u16)>,
    /// Optional path to persist and load server list.
    pub server_list_path: Option<PathBuf>,
    /// Whether runtime server-list updates should replace configured endpoints.
    pub follow_server_list_updates: bool,
    /// Delay between reconnection attempts (in seconds).
    pub reconnect_delay_secs: u64,
    /// Timeout for establishing connections (in seconds).
    pub connection_timeout_secs: u64,
    /// Timeout for outbound socket writes (in seconds).
    pub write_timeout_secs: u64,
    /// Timeout for watchdog health checks (in seconds).
    pub watchdog_timeout_secs: u64,
    /// Maximum number of exceptions before triggering watchdog timeout.
    pub max_exceptions: u32,
    /// Decoder configuration.
    pub decode: QbtDecodeConfig,
}

impl QbtReceiverConfig {
    /// Validates the configuration.
    ///
    /// # Errors
    ///
    /// Returns `QbtReceiverConfigError::EmptyEmail` if email is empty or whitespace.
    /// Returns `QbtReceiverConfigError::NoServers` if no servers are configured.
    pub fn validate(&self) -> Result<(), crate::qbt_receiver::error::QbtReceiverConfigError> {
        if self.email.trim().is_empty() {
            return Err(crate::qbt_receiver::error::QbtReceiverConfigError::EmptyEmail);
        }
        if self.servers.is_empty() {
            return Err(crate::qbt_receiver::error::QbtReceiverConfigError::NoServers);
        }
        if self.write_timeout_secs == 0 {
            return Err(crate::qbt_receiver::error::QbtReceiverConfigError::ZeroWriteTimeout);
        }
        Ok(())
    }
}
