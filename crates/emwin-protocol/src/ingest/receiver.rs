#![allow(missing_docs)]

//! Unified ingest receiver implementation.
//!
//! This module provides the [`IngestReceiver`] type which wraps either a QBT or Weather Wire
//! receiver and presents a unified interface for receiving products from either source.
//!
//! The receiver abstracts over protocol-specific details, allowing applications to work with
//! a single event stream regardless of the underlying transport.

use crate::ingest::model::{IngestError, IngestEvent};
use crate::runtime_support::ReceiverEventStream;
#[cfg(feature = "qbt")]
use crate::{
    ingest::qbt_adapter::adapt_qbt_events,
    qbt_receiver::{QbtReceiver, QbtReceiverClient, QbtReceiverConfig, QbtReceiverResult},
};
#[cfg(feature = "wxwire")]
use crate::{
    ingest::wxwire_adapter::adapt_wxwire_events,
    wxwire_receiver::{
        WxWireReceiver, WxWireReceiverClient, WxWireReceiverConfig, WxWireReceiverResult,
    },
};

/// Source-specific configuration for building an [`IngestReceiver`].
#[derive(Debug, Clone)]
pub enum IngestConfig {
    #[cfg(feature = "qbt")]
    Qbt(QbtReceiverConfig),
    #[cfg(feature = "wxwire")]
    WxWire(WxWireReceiverConfig),
}

/// Unified lifecycle wrapper over the supported receiver implementations.
pub struct IngestReceiver {
    inner: IngestReceiverInner,
}

enum IngestReceiverInner {
    #[cfg(feature = "qbt")]
    Qbt(QbtReceiver),
    #[cfg(feature = "wxwire")]
    WxWire(WxWireReceiver),
}

impl IngestReceiver {
    pub fn build(config: IngestConfig) -> Result<Self, IngestError> {
        let inner = match config {
            #[cfg(feature = "qbt")]
            IngestConfig::Qbt(config) => {
                IngestReceiverInner::Qbt(QbtReceiver::builder(config).build()?)
            }
            #[cfg(feature = "wxwire")]
            IngestConfig::WxWire(config) => {
                IngestReceiverInner::WxWire(WxWireReceiver::builder(config).build()?)
            }
        };

        Ok(Self { inner })
    }

    pub fn start(&mut self) -> Result<(), IngestError> {
        match &mut self.inner {
            #[cfg(feature = "qbt")]
            IngestReceiverInner::Qbt(receiver) => start_qbt(receiver).map_err(IngestError::from),
            #[cfg(feature = "wxwire")]
            IngestReceiverInner::WxWire(receiver) => {
                start_wxwire(receiver).map_err(IngestError::from)
            }
        }
    }

    pub fn events(&mut self) -> Result<ReceiverEventStream<IngestEvent, IngestError>, IngestError> {
        match &mut self.inner {
            #[cfg(feature = "qbt")]
            IngestReceiverInner::Qbt(receiver) => {
                Ok(Box::pin(adapt_qbt_events(receiver.events()?)))
            }
            #[cfg(feature = "wxwire")]
            IngestReceiverInner::WxWire(receiver) => {
                Ok(Box::pin(adapt_wxwire_events(receiver.events()?)))
            }
        }
    }

    pub async fn stop(&mut self) -> Result<(), IngestError> {
        match &mut self.inner {
            #[cfg(feature = "qbt")]
            IngestReceiverInner::Qbt(receiver) => receiver.stop().await.map_err(IngestError::from),
            #[cfg(feature = "wxwire")]
            IngestReceiverInner::WxWire(receiver) => {
                receiver.stop().await.map_err(IngestError::from)
            }
        }
    }
}

#[cfg(feature = "qbt")]
fn start_qbt(receiver: &mut QbtReceiver) -> QbtReceiverResult<()> {
    receiver.start()
}

#[cfg(feature = "wxwire")]
fn start_wxwire(receiver: &mut WxWireReceiver) -> WxWireReceiverResult<()> {
    receiver.start()
}
