#![allow(missing_docs)]

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// A fully assembled Weather Wire product file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct WxWireReceiverFile {
    /// Filename synthesized from weather product metadata.
    pub filename: String,
    /// Raw NOAAPort-formatted payload bytes.
    pub data: Bytes,
    /// Product subject/body summary.
    pub subject: String,
    /// Product identifier.
    pub id: String,
    /// Product issue timestamp in UTC.
    pub issue_utc: SystemTime,
    /// WMO TTAAII code.
    pub ttaaii: String,
    /// Issuing center code.
    pub cccc: String,
    /// AWIPS product ID.
    pub awipsid: String,
    /// Delay stamp provided by the feed, when present.
    pub delay_stamp_utc: Option<SystemTime>,
}

/// Warning events produced by weather wire decode/runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum WxWireReceiverWarning {
    /// Expected NWWS namespace stanza was missing.
    MissingNwwsNamespace,
    /// Stanza body was empty.
    EmptyBody,
    /// Timestamp could not be parsed and fallback was used.
    TimestampParseFallback {
        /// Raw timestamp input.
        raw: String,
    },
    /// Decoder recovered from a malformed stanza.
    DecoderRecovered {
        /// Error string used for diagnostics.
        error: String,
    },
    /// Event handler returned an error.
    HandlerError {
        /// Handler error message.
        message: String,
    },
    /// Events were dropped because event queue was full.
    BackpressureDrop {
        /// Dropped events since last warning emission.
        dropped_since_last_report: u64,
        /// Total dropped events since runtime start.
        total_dropped_events: u64,
    },
    /// XMPP transport reported an error.
    TransportError {
        /// Underlying transport error message.
        message: String,
    },
}

/// Frame events emitted by the weather wire decoder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum WxWireReceiverFrameEvent {
    /// Fully assembled weather product file.
    File(WxWireReceiverFile),
    /// Non-fatal warning.
    Warning(WxWireReceiverWarning),
}
