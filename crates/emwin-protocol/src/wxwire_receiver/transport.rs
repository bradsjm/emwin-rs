use crate::wxwire_receiver::config::{WXWIRE_PORT, WXWIRE_ROOM};
use crate::wxwire_receiver::error::{
    WxWireReceiverError, WxWireReceiverResult, WxWireTransportError,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use minidom::Element;
use quick_xml::Reader;
use quick_xml::events::Event as XmlEvent;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::crypto::ring;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tracing::{debug, warn};
use xmpp_parsers::jid::BareJid;

const CLIENT_NS: &str = "jabber:client";
const SM3_NS: &str = "urn:xmpp:sm:3";
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);
const MAX_READ_BUFFER_BYTES: usize = 1024 * 1024;

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

#[derive(Debug)]
enum XmppSocket {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl XmppSocket {
    async fn read_some(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.read(buf).await,
            Self::Tls(stream) => stream.read(buf).await,
        }
    }

    async fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match self {
            Self::Plain(stream) => stream.write_all(bytes).await,
            Self::Tls(stream) => stream.write_all(bytes).await,
        }
    }

    async fn shutdown(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(stream) => stream.shutdown().await,
            Self::Tls(stream) => stream.shutdown().await,
        }
    }
}

/// Minimal XMPP transport with only the functionality needed for NWWS product reception.
#[derive(Debug)]
pub struct XmppWxWireTransport {
    socket: Option<XmppSocket>,
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
        let connect_deadline = Instant::now()
            .checked_add(connect_timeout)
            .ok_or(WxWireTransportError::ConnectTimeoutOverflow)?;
        let addr = format!("{endpoint_host}:{WXWIRE_PORT}");
        let tcp = tokio::time::timeout(
            remaining_connect_timeout(connect_deadline)?,
            TcpStream::connect(addr.as_str()),
        )
        .await
        .map_err(|_| WxWireTransportError::ConnectTimeout)
        .and_then(|result| {
            result.map_err(|err| WxWireTransportError::TcpConnect(err.to_string()))
        })?;

        let room_bare = BareJid::from_str(WXWIRE_ROOM)
            .map_err(|err| WxWireTransportError::InvalidRoomJid(err.to_string()))?;

        let mut session = XmppSession::new(endpoint_host.to_string(), XmppSocket::Plain(tcp));
        session
            .open_stream(remaining_connect_timeout(connect_deadline)?)
            .await?;

        let features = session
            .wait_for_tag(
                "stream:features",
                remaining_connect_timeout(connect_deadline)?,
            )
            .await?;
        if !features.contains("urn:ietf:params:xml:ns:xmpp-tls") {
            return Err(WxWireTransportError::MissingStartTls.into());
        }

        session
            .send_raw("<starttls xmlns='urn:ietf:params:xml:ns:xmpp-tls'/>")
            .await?;
        let proceed = session
            .wait_for_tag("proceed", remaining_connect_timeout(connect_deadline)?)
            .await?;
        if !proceed.contains("urn:ietf:params:xml:ns:xmpp-tls") {
            return Err(WxWireTransportError::StartTlsRejected.into());
        }

        session
            .upgrade_tls(remaining_connect_timeout(connect_deadline)?)
            .await?;
        session
            .open_stream(remaining_connect_timeout(connect_deadline)?)
            .await?;

        let sasl_features = session
            .wait_for_tag(
                "stream:features",
                remaining_connect_timeout(connect_deadline)?,
            )
            .await?;
        if !sasl_features.contains("urn:ietf:params:xml:ns:xmpp-sasl") {
            return Err(WxWireTransportError::MissingSaslMechanisms.into());
        }

        let auth_payload = BASE64_STANDARD.encode(format!("\0{username}\0{password}"));
        let auth = format!(
            "<auth xmlns='urn:ietf:params:xml:ns:xmpp-sasl' mechanism='PLAIN'>{auth_payload}</auth>"
        );
        session.send_raw(auth.as_str()).await?;

        let sasl_reply = session
            .wait_for_any_tag(
                &["success", "failure"],
                remaining_connect_timeout(connect_deadline)?,
            )
            .await?;
        if sasl_reply.contains("<failure") {
            return Err(WxWireTransportError::AuthenticationFailed(sasl_reply).into());
        }

        session
            .open_stream(remaining_connect_timeout(connect_deadline)?)
            .await?;
        let post_auth_features = session
            .wait_for_tag(
                "stream:features",
                remaining_connect_timeout(connect_deadline)?,
            )
            .await?;
        if !post_auth_features.contains("urn:ietf:params:xml:ns:xmpp-bind") {
            return Err(WxWireTransportError::MissingResourceBinding.into());
        }

        let bind_id = format!("bb-bind-{}", chrono_like_suffix());
        let bind_iq = format!(
            "<iq type='set' id='{bind_id}'><bind xmlns='urn:ietf:params:xml:ns:xmpp-bind'/></iq>"
        );
        session.send_raw(bind_iq.as_str()).await?;

        let bind_result = session
            .wait_for_tag("iq", remaining_connect_timeout(connect_deadline)?)
            .await?;
        if !bind_result.contains(format!("id=\"{bind_id}\"").as_str())
            && !bind_result.contains(format!("id='{bind_id}'").as_str())
        {
            return Err(WxWireTransportError::UnexpectedBindResponse(bind_result).into());
        }
        if !bind_result.contains("type='result'") && !bind_result.contains("type=\"result\"") {
            return Err(WxWireTransportError::ResourceBindFailed(bind_result).into());
        }

        let sm_enabled = if post_auth_features.contains(SM3_NS) {
            session
                .send_raw("<enable xmlns='urn:xmpp:sm:3' resume='true'/>")
                .await?;
            let sm_reply = session
                .wait_for_any_tag(
                    &["enabled", "failed"],
                    remaining_connect_timeout(connect_deadline)?,
                )
                .await?;
            if sm_reply.contains("<failed") {
                return Err(WxWireTransportError::StreamManagementEnableFailed(sm_reply).into());
            }
            true
        } else {
            false
        };

        let nick = format!("bb{}", chrono_like_suffix());
        let join = format!(
            "<presence to='{WXWIRE_ROOM}/{nick}'><x xmlns='http://jabber.org/protocol/muc'><history maxstanzas='25'/></x></presence>"
        );
        session.send_raw(join.as_str()).await?;

        let join_confirm =
            tokio::time::timeout(remaining_connect_timeout(connect_deadline)?, async {
                loop {
                    let stanza = session
                        .wait_for_tag("presence", remaining_connect_timeout(connect_deadline)?)
                        .await?;
                    if is_room_join_presence(stanza.as_str(), &room_bare, nick.as_str())? {
                        return Ok::<String, WxWireReceiverError>(stanza);
                    }
                }
            })
            .await
            .map_err(|_| WxWireTransportError::JoinConfirmationTimeout)??;

        if join_confirm.contains("type='error'") || join_confirm.contains("type=\"error\"") {
            return Err(WxWireTransportError::JoinRejected(join_confirm).into());
        }

        Ok(Self {
            socket: session.socket,
            room_bare,
            label: format!("{endpoint_host}:{WXWIRE_PORT} room={WXWIRE_ROOM}"),
            read_buf: session.read_buf,
            sm_enabled,
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

#[derive(Debug)]
struct XmppSession {
    endpoint_host: String,
    socket: Option<XmppSocket>,
    read_buf: String,
}

impl XmppSession {
    fn new(endpoint_host: String, socket: XmppSocket) -> Self {
        Self {
            endpoint_host,
            socket: Some(socket),
            read_buf: String::new(),
        }
    }

    async fn send_raw(&mut self, xml: &str) -> WxWireReceiverResult<()> {
        self.socket
            .as_mut()
            .ok_or(WxWireTransportError::SocketNotAvailable)?
            .write_all(xml.as_bytes())
            .await
            .map_err(|err| WxWireTransportError::WriteFailed(err.to_string()).into())
    }

    async fn open_stream(&mut self, _timeout: Duration) -> WxWireReceiverResult<()> {
        let open = format!(
            "<?xml version='1.0' encoding='utf-8'?><stream:stream xmlns='jabber:client' xmlns:stream='http://etherx.jabber.org/streams' to='{}' version='1.0'>",
            self.endpoint_host
        );
        self.send_raw(open.as_str()).await
    }

    async fn upgrade_tls(&mut self, timeout: Duration) -> WxWireReceiverResult<()> {
        let plain = match self
            .socket
            .take()
            .ok_or(WxWireTransportError::SocketNotAvailable)?
        {
            XmppSocket::Plain(stream) => stream,
            XmppSocket::Tls(stream) => {
                self.socket = Some(XmppSocket::Tls(stream));
                return Ok(());
            }
        };

        let mut roots = RootCertStore::empty();
        for cert in rustls_native_certs::load_native_certs().certs {
            let _ = roots.add(cert);
        }

        let config = ClientConfig::builder_with_provider(Arc::new(ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|err| WxWireTransportError::TlsHandshakeFailed(err.to_string()))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector = TlsConnector::from(std::sync::Arc::new(config));
        let server_name = ServerName::try_from(self.endpoint_host.clone())
            .map_err(|_| WxWireTransportError::InvalidTlsServerName)?;

        let tls = tokio::time::timeout(timeout, connector.connect(server_name, plain))
            .await
            .map_err(|_| WxWireTransportError::TlsHandshakeTimeout)
            .and_then(|result| {
                result.map_err(|err| WxWireTransportError::TlsHandshakeFailed(err.to_string()))
            })?;

        self.socket = Some(XmppSocket::Tls(Box::new(tls)));
        self.read_buf.clear();
        Ok(())
    }

    async fn read_more(&mut self, timeout: Duration) -> WxWireReceiverResult<()> {
        let mut buf = [0u8; 8192];
        let socket = self
            .socket
            .as_mut()
            .ok_or(WxWireTransportError::SocketNotAvailable)?;
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
            "xmpp handshake read buffer exceeded limit",
        )?;
        Ok(())
    }

    async fn wait_for_tag(&mut self, tag: &str, timeout: Duration) -> WxWireReceiverResult<String> {
        self.wait_for_any_tag(&[tag], timeout).await
    }

    async fn wait_for_any_tag(
        &mut self,
        tags: &[&str],
        timeout: Duration,
    ) -> WxWireReceiverResult<String> {
        let wait_label = tags.join(" or ");
        loop {
            while let Some(elem) = pop_next_top_level_element(&mut self.read_buf) {
                if tags.iter().any(|tag| {
                    stanza_root_tag_name(elem.as_str())
                        .as_deref()
                        .map(|name| name == *tag)
                        .unwrap_or(false)
                }) {
                    return Ok(elem);
                }
            }
            self.read_more(timeout)
                .await
                .map_err(|err| attach_handshake_timeout_context(err, wait_label.as_str()))?;
        }
    }
}

fn attach_handshake_timeout_context(
    err: WxWireReceiverError,
    waiting_for: &str,
) -> WxWireReceiverError {
    match err {
        WxWireReceiverError::Transport(WxWireTransportError::ReadTimeout) => {
            WxWireTransportError::ReadTimeoutWaiting(waiting_for.to_string()).into()
        }
        other => other,
    }
}

fn chrono_like_suffix() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", now.as_secs())
}

fn remaining_connect_timeout(deadline: Instant) -> WxWireReceiverResult<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(|| WxWireTransportError::ConnectTimeout.into())
}

fn append_with_read_limit(
    read_buf: &mut String,
    chunk: &str,
    overflow_message: &str,
) -> WxWireReceiverResult<()> {
    if read_buf.len().saturating_add(chunk.len()) > MAX_READ_BUFFER_BYTES {
        read_buf.clear();
        return Err(WxWireTransportError::BufferOverflow(overflow_message.to_string()).into());
    }
    read_buf.push_str(chunk);
    Ok(())
}

fn pop_next_top_level_element(buf: &mut String) -> Option<String> {
    if buf.is_empty() {
        return None;
    }

    let mut reader = Reader::from_str(buf.as_str());
    reader.config_mut().trim_text(false);

    let mut depth: usize = 0;
    let mut root_start: Option<usize> = None;
    let mut last_pos: usize = 0;

    loop {
        let start = last_pos;
        let event = match reader.read_event() {
            Ok(event) => event,
            Err(err) => {
                if err.to_string().contains("Unexpected EOF") {
                    return None;
                }
                let recover_from = start.saturating_add(1);
                if recover_from < buf.len()
                    && let Some(offset) = buf[recover_from..].find('<')
                {
                    buf.drain(..recover_from + offset);
                } else {
                    buf.clear();
                }
                return None;
            }
        };
        let end = usize::try_from(reader.buffer_position()).unwrap_or(buf.len());
        last_pos = end;

        match event {
            XmlEvent::Start(start_event) => {
                let name_buf = start_event.name().as_ref().to_vec();
                let Ok(name) = std::str::from_utf8(name_buf.as_slice()) else {
                    buf.drain(..end);
                    return None;
                };
                if depth == 0 && name == "stream:stream" {
                    buf.drain(..end);
                    return pop_next_top_level_element(buf);
                }
                if depth == 0 {
                    root_start = Some(start);
                }
                depth = depth.saturating_add(1);
            }
            XmlEvent::Empty(start_event) => {
                let name_buf = start_event.name().as_ref().to_vec();
                let Ok(name) = std::str::from_utf8(name_buf.as_slice()) else {
                    buf.drain(..end);
                    return None;
                };
                if depth == 0 && name == "stream:stream" {
                    buf.drain(..end);
                    return pop_next_top_level_element(buf);
                }
                if depth == 0 {
                    let stanza = buf[start..end].to_string();
                    buf.drain(..end);
                    return Some(stanza);
                }
            }
            XmlEvent::End(_) => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0
                        && let Some(root) = root_start.take()
                    {
                        let stanza = buf[root..end].to_string();
                        buf.drain(..end);
                        return Some(stanza);
                    }
                } else {
                    buf.drain(..end);
                    return None;
                }
            }
            XmlEvent::Text(text) => {
                if depth == 0 && !text.as_ref().iter().all(|b| b.is_ascii_whitespace()) {
                    buf.drain(..end);
                    return pop_next_top_level_element(buf);
                }
            }
            XmlEvent::Decl(_)
            | XmlEvent::PI(_)
            | XmlEvent::Comment(_)
            | XmlEvent::DocType(_)
            | XmlEvent::GeneralRef(_)
            | XmlEvent::CData(_) => {}
            XmlEvent::Eof => return None,
        }
    }
}

fn stanza_root_tag_name(stanza: &str) -> Option<String> {
    let mut reader = Reader::from_str(stanza);
    reader.config_mut().trim_text(false);

    loop {
        match reader.read_event() {
            Ok(XmlEvent::Start(start_event)) => {
                let name_buf = start_event.name().as_ref().to_vec();
                return std::str::from_utf8(name_buf.as_slice())
                    .ok()
                    .map(ToString::to_string);
            }
            Ok(XmlEvent::Empty(start_event)) => {
                let name_buf = start_event.name().as_ref().to_vec();
                return std::str::from_utf8(name_buf.as_slice())
                    .ok()
                    .map(ToString::to_string);
            }
            Ok(
                XmlEvent::Decl(_)
                | XmlEvent::PI(_)
                | XmlEvent::Comment(_)
                | XmlEvent::DocType(_)
                | XmlEvent::GeneralRef(_)
                | XmlEvent::CData(_)
                | XmlEvent::Text(_),
            ) => {}
            Ok(XmlEvent::End(_) | XmlEvent::Eof) | Err(_) => return None,
        }
    }
}

fn is_supported_top_level_stanza(stanza: &str) -> bool {
    matches!(
        stanza_root_tag_name(stanza).as_deref(),
        Some("message" | "presence" | "iq" | "r" | "a")
    )
}

fn is_room_join_presence(
    stanza: &str,
    room_bare: &BareJid,
    nick: &str,
) -> WxWireReceiverResult<bool> {
    let element = parse_element_with_default_ns(stanza)
        .map_err(|err| WxWireTransportError::InvalidJoinPresence(err.to_string()))?;

    if element.name() != "presence" {
        return Ok(false);
    }

    if element.attr("type") == Some("error") || element.attr("type") == Some("unavailable") {
        return Ok(false);
    }

    let Some(from) = element.attr("from") else {
        return Ok(false);
    };

    let Some((bare, resource)) = from.split_once('/') else {
        return Ok(false);
    };

    Ok(bare == room_bare.to_string() && resource == nick)
}

fn parse_element_with_default_ns(xml: &str) -> Result<Element, minidom::Error> {
    match xml.parse::<Element>() {
        Ok(element) => Ok(element),
        Err(_) => add_default_client_ns(xml).parse::<Element>(),
    }
}

fn add_default_client_ns(xml: &str) -> String {
    let Some(open_start) = xml.find('<') else {
        return xml.to_string();
    };
    let Some(open_end_rel) = xml[open_start..].find('>') else {
        return xml.to_string();
    };
    let open_end = open_start + open_end_rel;
    let open_tag = &xml[open_start..=open_end];
    if open_tag.starts_with("</") || open_tag.starts_with("<?") || open_tag.starts_with("<!") {
        return xml.to_string();
    }
    if open_tag.contains("xmlns=") || open_tag.contains("xmlns:") {
        return xml.to_string();
    }

    let insert_at = if open_tag.ends_with("/>") {
        open_end - 1
    } else {
        open_end
    };

    let mut out = String::with_capacity(xml.len() + CLIENT_NS.len() + 16);
    out.push_str(&xml[..insert_at]);
    out.push_str(" xmlns='");
    out.push_str(CLIENT_NS);
    out.push('\'');
    out.push_str(&xml[insert_at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::{
        add_default_client_ns, append_with_read_limit, is_room_join_presence,
        is_supported_top_level_stanza, pop_next_top_level_element, stanza_root_tag_name,
    };
    use crate::wxwire_receiver::error::{WxWireReceiverError, WxWireTransportError};
    use std::str::FromStr;
    use xmpp_parsers::jid::BareJid;

    #[test]
    fn pop_next_top_level_element_returns_first_complete_match() {
        let mut s = "abc<presence from='a/b'></presence><message>x</message>".to_string();
        let presence = pop_next_top_level_element(&mut s).expect("presence present");
        assert!(presence.starts_with("<presence"));
        assert!(s.contains("<message>"));
    }

    #[test]
    fn pop_next_top_level_element_handles_nested_self_closing_tag() {
        let mut s = "<stream:features><ver xmlns='urn:xmpp:features:rosterver'/><bind xmlns='urn:ietf:params:xml:ns:xmpp-bind'/></stream:features><iq/>".to_string();
        let features = pop_next_top_level_element(&mut s).expect("features present");
        assert!(features.contains("xmpp-bind"));
        assert!(features.ends_with("</stream:features>"));
        assert!(s.starts_with("<iq/>"));
    }

    #[test]
    fn pop_next_top_level_element_skips_stream_open_tag() {
        let mut s = "<stream:stream xmlns:stream='http://etherx.jabber.org/streams'><presence/>"
            .to_string();
        let presence = pop_next_top_level_element(&mut s).expect("presence present");
        assert_eq!(presence, "<presence/>");
    }

    #[test]
    fn pop_next_top_level_element_returns_features_from_same_buffer_as_stream_open() {
        let mut s = "<stream:stream xmlns:stream='http://etherx.jabber.org/streams'><stream:features><bind xmlns='urn:ietf:params:xml:ns:xmpp-bind'/></stream:features>".to_string();
        let features = pop_next_top_level_element(&mut s).expect("features present");
        assert!(features.starts_with("<stream:features"));
        assert!(features.contains("xmpp-bind"));
    }

    #[test]
    fn room_join_presence_requires_room_and_nick_match() {
        let room = BareJid::from_str("nwws@conference.nwws-oi.weather.gov").expect("valid room");
        let stanza =
            "<presence xmlns='jabber:client' from='nwws@conference.nwws-oi.weather.gov/bb123'/>";
        assert!(is_room_join_presence(stanza, &room, "bb123").expect("parse ok"));
        assert!(!is_room_join_presence(stanza, &room, "bb999").expect("parse ok"));
    }

    #[test]
    fn room_join_presence_without_xmlns_is_accepted() {
        let room = BareJid::from_str("nwws@conference.nwws-oi.weather.gov").expect("valid room");
        let stanza = "<presence from='nwws@conference.nwws-oi.weather.gov/bb123'/>";
        assert!(is_room_join_presence(stanza, &room, "bb123").expect("parse ok"));
    }

    #[test]
    fn add_default_client_ns_inserts_namespace_once() {
        let xml = "<presence from='a@b/c'/>";
        let patched = add_default_client_ns(xml);
        assert!(patched.contains("xmlns='jabber:client'"));
        assert_eq!(patched.matches("xmlns=").count(), 1);
    }

    #[test]
    fn add_default_client_ns_keeps_child_namespace_and_adds_root_namespace() {
        let xml =
            "<presence from='a@b/c'><x xmlns='http://jabber.org/protocol/muc#user'/></presence>";
        let patched = add_default_client_ns(xml);
        assert!(patched.starts_with("<presence "));
        assert!(patched.contains("xmlns='jabber:client'"));
        assert!(patched.contains("xmlns='http://jabber.org/protocol/muc#user'"));
    }

    #[test]
    fn stanza_root_tag_name_recognizes_prefixed_features_without_local_xmlns() {
        let stanza = "<stream:features><starttls xmlns='urn:ietf:params:xml:ns:xmpp-tls'/></stream:features>";
        assert_eq!(
            stanza_root_tag_name(stanza).as_deref(),
            Some("stream:features")
        );
    }

    #[test]
    fn stanza_root_tag_name_recognizes_message_root() {
        let stanza = "<message type='groupchat'><body>hello</body></message>";
        assert_eq!(stanza_root_tag_name(stanza).as_deref(), Some("message"));
    }

    #[test]
    fn supported_top_level_stanza_accepts_xmpp_roots_only() {
        assert!(is_supported_top_level_stanza("<message type='groupchat'/>"));
        assert!(is_supported_top_level_stanza("<presence/>"));
        assert!(is_supported_top_level_stanza("<iq/>"));
        assert!(is_supported_top_level_stanza("<r xmlns='urn:xmpp:sm:3'/>"));
        assert!(is_supported_top_level_stanza("<a xmlns='urn:xmpp:sm:3'/>"));
        assert!(!is_supported_top_level_stanza("<site id='YRBA2'></site>"));
    }

    #[test]
    fn append_with_read_limit_rejects_oversized_chunk() {
        let mut buf = "x".repeat((1024 * 1024) - 4);
        let err = append_with_read_limit(&mut buf, "12345", "buffer too large")
            .expect_err("chunk should exceed max read buffer size");

        assert!(matches!(
            err,
            WxWireReceiverError::Transport(WxWireTransportError::BufferOverflow(message))
            if message == "buffer too large"
        ));
        assert!(buf.is_empty());
    }
}
