//! Data models for the unified ingest receiver.
//!
//! This module defines the core types for receiving products from either QBT or Weather Wire
//! sources, including product events, telemetry, and warnings.

#![allow(missing_docs)]

#[cfg(feature = "qbt")]
use crate::qbt_receiver::{
    QbtCompletedFile, QbtProtocolWarning, QbtReceiverError, QbtReceiverTelemetrySnapshot,
};
#[cfg(feature = "wxwire")]
use crate::wxwire_receiver::{
    WxWireReceiverError, WxWireReceiverFile, WxWireReceiverTelemetrySnapshot, WxWireReceiverWarning,
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use thiserror::Error;

/// A complete product received from either QBT or Weather Wire source.
///
/// This type normalizes products from different sources into a common structure
/// for application consumption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ReceivedProduct {
    /// Product filename
    pub filename: String,
    /// Product data bytes
    pub data: Bytes,
    /// Timestamp when the product was issued by the source
    pub source_timestamp_utc: SystemTime,
    /// Source that provided this product
    pub origin: ProductOrigin,
}

/// Source that provided a product.
///
/// Distinguishes between QBT TCP and Weather Wire sources with
/// source-specific metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ProductOrigin {
    /// Product from QBT receiver
    #[cfg(feature = "qbt")]
    Qbt,
    /// Product from Weather Wire receiver with metadata
    #[cfg(feature = "wxwire")]
    WxWire {
        /// XMPP message ID
        message_id: String,
        /// Subject from XMPP message
        subject: String,
        /// Delay stamp if present in message
        delay_stamp_utc: Option<SystemTime>,
    },
}

/// Unified event stream from the ingest receiver.
///
/// Combines product events with telemetry and warnings into a single stream
/// for application consumption.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum IngestEvent {
    /// A complete product was received
    Product(ReceivedProduct),
    /// Connection established to upstream endpoint
    Connected { endpoint: String },
    /// Connection lost or disconnected
    Disconnected,
    /// Telemetry snapshot from receiver
    Telemetry(IngestTelemetry),
    /// Warning or non-fatal error
    Warning(IngestWarning),
}

/// Telemetry snapshot from receiver.
///
/// Contains receiver-specific metrics for either QBT or Weather Wire.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum IngestTelemetry {
    /// QBT receiver telemetry
    #[cfg(feature = "qbt")]
    Qbt(QbtReceiverTelemetrySnapshot),
    /// Weather Wire receiver telemetry
    #[cfg(feature = "wxwire")]
    WxWire(WxWireReceiverTelemetrySnapshot),
}

/// Warning or non-fatal error from receiver.
///
/// Indicates a problem that did not prevent operation but may be of interest.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum IngestWarning {
    /// QBT protocol warning
    #[cfg(feature = "qbt")]
    Qbt(QbtProtocolWarning),
    /// Weather Wire warning
    #[cfg(feature = "wxwire")]
    WxWire(WxWireReceiverWarning),
}

/// Errors from the ingest receiver.
///
/// Wraps receiver-specific errors for unified error handling.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum IngestError {
    #[cfg(feature = "qbt")]
    #[error("QBT receiver error: {0}")]
    Qbt(#[from] QbtReceiverError),
    #[cfg(feature = "wxwire")]
    #[error("Weather Wire receiver error: {0}")]
    WxWire(#[from] WxWireReceiverError),
}

#[cfg(feature = "qbt")]
impl From<QbtCompletedFile> for ReceivedProduct {
    fn from(value: QbtCompletedFile) -> Self {
        Self {
            filename: value.filename,
            data: value.data,
            source_timestamp_utc: value.timestamp_utc,
            origin: ProductOrigin::Qbt,
        }
    }
}

#[cfg(feature = "wxwire")]
impl From<WxWireReceiverFile> for ReceivedProduct {
    fn from(value: WxWireReceiverFile) -> Self {
        Self {
            filename: value.filename,
            data: value.data,
            source_timestamp_utc: value.issue_utc,
            origin: ProductOrigin::WxWire {
                message_id: value.id,
                subject: value.subject,
                delay_stamp_utc: value.delay_stamp_utc,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ProductOrigin, ReceivedProduct};
    #[cfg(feature = "qbt")]
    use crate::qbt_receiver::QbtCompletedFile;
    #[cfg(feature = "wxwire")]
    use crate::wxwire_receiver::WxWireReceiverFile;
    use bytes::Bytes;
    use std::time::{Duration, SystemTime};

    #[cfg(feature = "qbt")]
    #[test]
    fn qbt_completed_file_converts_to_received_product() {
        let timestamp = SystemTime::UNIX_EPOCH + Duration::from_secs(123);
        let product: ReceivedProduct = QbtCompletedFile {
            filename: "A.TXT".to_string(),
            data: Bytes::from_static(b"abc"),
            timestamp_utc: timestamp,
        }
        .into();

        assert_eq!(product.filename, "A.TXT");
        assert_eq!(product.data, Bytes::from_static(b"abc"));
        assert_eq!(product.source_timestamp_utc, timestamp);
        assert!(matches!(product.origin, ProductOrigin::Qbt));
    }

    #[cfg(feature = "wxwire")]
    #[test]
    fn wxwire_file_converts_to_received_product() {
        let issue = SystemTime::UNIX_EPOCH + Duration::from_secs(111);
        let delay = Some(SystemTime::UNIX_EPOCH + Duration::from_secs(222));
        let product: ReceivedProduct = WxWireReceiverFile {
            filename: "B.TXT".to_string(),
            data: Bytes::from_static(b"xyz"),
            subject: "subject".to_string(),
            id: "id-1".to_string(),
            issue_utc: issue,
            ttaaii: "TTAAII".to_string(),
            cccc: "KAAA".to_string(),
            awipsid: "AFDXXX".to_string(),
            delay_stamp_utc: delay,
        }
        .into();

        assert_eq!(product.filename, "B.TXT");
        assert_eq!(product.data, Bytes::from_static(b"xyz"));
        assert_eq!(product.source_timestamp_utc, issue);
        assert!(matches!(
            product.origin,
            ProductOrigin::WxWire {
                message_id,
                subject,
                delay_stamp_utc,
            } if message_id == "id-1" && subject == "subject" && delay_stamp_utc == delay
        ));
    }
}
