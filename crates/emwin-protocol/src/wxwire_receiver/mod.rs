//! Weather Wire receiver runtime built on a focused XMPP transport.
//!
//! This module combines the custom transport, decoder, and reconnect logic needed for NWWS/Weather
//! Wire reception. It keeps XMPP details local and exposes the same style of runtime API as the
//! QBT receiver.
//!
//! Module ownership is split by responsibility:
//! - `client`: connection lifecycle, reconnect behavior, and event dispatch
//! - `transport`: XMPP stream framing, stanza buffering, and room-join flow
//! - `codec`: Weather Wire stanza decoding into typed frame events
//! - `model`: file, frame, and warning payload types
//! - `config`: validated receiver configuration
//! - `error`: typed runtime errors

mod client;
mod codec;
mod config;
mod error;
mod model;
mod transport;

pub use client::{
    UnstableWxWireReceiverIngress, WxWireReceiver, WxWireReceiverBuilder, WxWireReceiverClient,
    WxWireReceiverEvent, WxWireReceiverEventHandler, WxWireReceiverTelemetrySnapshot,
};
pub use codec::{WxWireDecoder, WxWireFrameDecoder};
pub use config::{WXWIRE_PRIMARY_HOST, WxWireReceiverConfig};
pub use error::{WxWireReceiverError, WxWireReceiverResult};
pub use model::{WxWireReceiverFile, WxWireReceiverFrameEvent, WxWireReceiverWarning};
pub use transport::{WxWireTransport, XmppWxWireTransport};

#[allow(missing_docs)]
pub mod unstable {
    pub use super::client::UnstableWxWireReceiverIngress;
}
