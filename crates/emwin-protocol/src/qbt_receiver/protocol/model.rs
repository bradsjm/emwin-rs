//! Data models for the EMWIN protocol.
//!
//! This module defines the core data structures used throughout the protocol
//! layer, including segments, server lists, and event types.

#![allow(missing_docs)]

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::SystemTime;

/// Protocol version identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum QbtProtocolVersion {
    /// Version 1: Fixed 1024-byte body size.
    V1,
    /// Version 2: Variable body size with optional compression.
    V2,
}

/// A single data segment (block) from a file transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct QbtSegment {
    /// Name of the file this segment belongs to.
    pub filename: String,
    /// Block number within the file (1-indexed).
    pub block_number: u32,
    /// Total number of blocks in the file.
    pub total_blocks: u32,
    /// Raw content bytes of this block.
    pub content: Bytes,
    /// Checksum value from the frame header.
    pub checksum: u32,
    /// Length of the body in bytes.
    pub length: usize,
    /// Protocol version used for this segment.
    pub version: QbtProtocolVersion,
    /// Timestamp from the frame header (UTC).
    pub timestamp_utc: SystemTime,
    /// Source address of the segment (if known).
    pub source: Option<SocketAddr>,
}

/// List of available servers for connection.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct QbtServerList {
    /// EMWIN server endpoints as (host, port) tuples.
    pub servers: Vec<(String, u16)>,
}

/// Warning events that can occur during protocol processing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum QbtProtocolWarning {
    /// Checksum validation failed for a data block.
    ChecksumMismatch {
        /// Filename of the affected block.
        filename: String,
        /// Block number that failed validation.
        block_number: u32,
    },
    /// Decompression failed for a compressed block.
    DecompressionFailed {
        /// Filename of the affected block.
        filename: String,
        /// Block number that failed decompression.
        block_number: u32,
        /// Error message describing the failure.
        reason: String,
    },
    /// Decoder recovered from an error and continued processing.
    DecoderRecovered {
        /// Error message describing what was recovered from.
        error: String,
    },
    /// Server list entry could not be parsed.
    MalformedServerEntry {
        /// Raw entry string that failed parsing.
        entry: String,
    },
    /// Timestamp parsing failed, using fallback.
    TimestampParseFallback {
        /// Raw timestamp string that failed parsing.
        raw: String,
    },
    /// Event handler returned an error.
    HandlerError {
        /// Error message from the handler.
        message: String,
    },
    /// Events were dropped due to backpressure.
    BackpressureDrop {
        /// Number of events dropped since last report.
        dropped_since_last_report: u64,
        /// Total number of events dropped.
        total_dropped_events: u64,
        /// Number of decoder recovery events.
        decoder_recovery_events: u64,
    },
}

/// Events emitted by the protocol decoder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum QbtFrameEvent {
    /// A data block was successfully decoded.
    DataBlock(QbtSegment),
    /// Server list update received.
    ServerListUpdate(QbtServerList),
    /// Warning condition detected.
    Warning(QbtProtocolWarning),
}

/// Authentication message sent during connection.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct QbtAuthMessage {
    /// User email address for authentication.
    pub email: String,
}
