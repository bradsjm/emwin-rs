mod session;
mod stanza;

use self::session::{SM3_NS, append_with_read_limit, connect_session};
use self::stanza::{
    is_supported_top_level_stanza, parse_element_with_default_ns, pop_next_top_level_element,
    stanza_root_tag_name,
};
use crate::wxwire_receiver::error::{WxWireReceiverResult, WxWireTransportError};
use std::pin::Pin;
use std::time::{Duration, Instant};
use tracing::{debug, warn};
use xmpp_parsers::jid::BareJid;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);

/// Abstraction over weather wire transport.
pub trait WxWireTransport: Send {
    /// Label for diagnostics and client events.
    fn label(&self) -> String;

    /// Reads one next weather-wire groupchat message stanza.
    fn next_stanza<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn std::future::Future<Output = WxWireReceiverResult<String>> + Send + 'a>>;

    /// Disconnects and cleans up the transport.
    fn disconnect<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn std::future::Future<Output = WxWireReceiverResult<()>> + Send + 'a>>;
}

/// Minimal XMPP transport with only the functionality needed for NWWS product reception.
#[derive(Debug)]
pub struct XmppWxWireTransport {
    socket: Option<session::XmppSocket>,
    room_bare: BareJid,
    label: String,
    read_buf: String,
    sm_enabled: bool,
    sm_handled_stanzas: u64,
    last_heartbeat: Instant,
}

impl XmppWxWireTransport {
    /// Connects and joins the fixed room.
    pub async fn connect(
        endpoint_host: &str,
        username: &str,
        password: &str,
        connect_timeout: Duration,
    ) -> WxWireReceiverResult<Self> {
        let session = connect_session(endpoint_host, username, password, connect_timeout).await?;
        Ok(Self {
            socket: session.socket,
            room_bare: session.room_bare,
            label: session.label,
            read_buf: session.read_buf,
            sm_enabled: session.sm_enabled,
            sm_handled_stanzas: 0,
            last_heartbeat: Instant::now(),
        })
    }

    async fn read_more(&mut self, timeout: Duration) -> WxWireReceiverResult<()> {
        let socket = self
            .socket
            .as_mut()
            .ok_or(WxWireTransportError::ClientNotConnected)?;
        let mut buf = [0u8; 8192];
        let read = tokio::time::timeout(timeout, socket.read_some(&mut buf))
            .await
            .map_err(|_| WxWireTransportError::ReadTimeout)
            .and_then(|result| {
                result.map_err(|err| WxWireTransportError::ReadFailed(err.to_string()))
            })?;

        if read == 0 {
            return Err(WxWireTransportError::StreamEnded.into());
        }

        let chunk = String::from_utf8_lossy(&buf[..read]);
        append_with_read_limit(
            &mut self.read_buf,
            chunk.as_ref(),
            "xmpp read buffer exceeded limit",
        )?;
        Ok(())
    }

    async fn send_raw(&mut self, xml: &str) -> WxWireReceiverResult<()> {
        let socket = self
            .socket
            .as_mut()
            .ok_or(WxWireTransportError::ClientNotConnected)?;
        socket
            .write_all(xml.as_bytes())
            .await
            .map_err(|err| WxWireTransportError::WriteFailed(err.to_string()).into())
    }

    async fn maybe_send_heartbeat(&mut self) -> WxWireReceiverResult<()> {
        if !self.sm_enabled {
            return Ok(());
        }
        if self.last_heartbeat.elapsed() < HEARTBEAT_INTERVAL {
            return Ok(());
        }

        self.send_raw("<r xmlns='urn:xmpp:sm:3'/>").await?;
        self.last_heartbeat = Instant::now();
        debug!("sent xmpp sm heartbeat request");
        Ok(())
    }
}

impl WxWireTransport for XmppWxWireTransport {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn next_stanza<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn std::future::Future<Output = WxWireReceiverResult<String>> + Send + 'a>> {
        Box::pin(async move {
            loop {
                if let Some(stanza) = pop_next_top_level_element(&mut self.read_buf) {
                    if !is_supported_top_level_stanza(stanza.as_str()) {
                        debug!(
                            root = stanza_root_tag_name(stanza.as_str())
                                .as_deref()
                                .unwrap_or("unknown"),
                            "ignoring non-xmpp top-level element"
                        );
                        continue;
                    }
                    let Ok(element) = parse_element_with_default_ns(stanza.as_str()) else {
                        warn!(
                            root = stanza_root_tag_name(stanza.as_str())
                                .as_deref()
                                .unwrap_or("unknown"),
                            "dropping unparsable xmpp top-level stanza"
                        );
                        continue;
                    };
                    if element.name() == "r" && element.ns() == SM3_NS {
                        let ack = format!("<a xmlns='{SM3_NS}' h='{}'/>", self.sm_handled_stanzas);
                        self.send_raw(ack.as_str()).await?;
                        continue;
                    }
                    if element.name() == "a" && element.ns() == SM3_NS {
                        continue;
                    }
                    if matches!(element.name(), "message" | "presence" | "iq") {
                        self.sm_handled_stanzas = self.sm_handled_stanzas.saturating_add(1);
                    }
                    if element.name() != "message" {
                        continue;
                    }
                    let type_ok = element.attr("type") == Some("groupchat");
                    if !type_ok {
                        continue;
                    }
                    let from_room = element
                        .attr("from")
                        .and_then(|from| from.split_once('/').map(|(bare, _)| bare.to_string()))
                        .map(|bare| bare == self.room_bare.to_string())
                        .unwrap_or(false);
                    if !from_room {
                        continue;
                    }
                    return Ok(stanza);
                }

                self.maybe_send_heartbeat().await?;
                self.read_more(Duration::from_secs(5)).await?;
            }
        })
    }

    fn disconnect<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn std::future::Future<Output = WxWireReceiverResult<()>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(mut socket) = self.socket.take() {
                let _ = socket.write_all(b"</stream:stream>").await;
                let _ = socket.shutdown().await;
            }
            Ok(())
        })
    }
}
