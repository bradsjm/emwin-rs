use super::stanza::is_room_join_presence;
use crate::wxwire_receiver::config::{WXWIRE_PORT, WXWIRE_ROOM};
use crate::wxwire_receiver::error::{
    WxWireReceiverError, WxWireReceiverResult, WxWireTransportError,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::crypto::ring;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use xmpp_parsers::jid::BareJid;

const MAX_READ_BUFFER_BYTES: usize = 1024 * 1024;
pub(super) const SM3_NS: &str = "urn:xmpp:sm:3";

#[derive(Debug)]
pub(super) struct ConnectedSession {
    pub(super) socket: Option<XmppSocket>,
    pub(super) room_bare: BareJid,
    pub(super) label: String,
    pub(super) read_buf: String,
    pub(super) sm_enabled: bool,
}

pub(super) async fn connect_session(
    endpoint_host: &str,
    username: &str,
    password: &str,
    connect_timeout: Duration,
    write_timeout: Duration,
) -> WxWireReceiverResult<ConnectedSession> {
    let connect_deadline = Instant::now()
        .checked_add(connect_timeout)
        .ok_or(WxWireTransportError::ConnectTimeoutOverflow)?;
    let mut session =
        XmppSession::connect(endpoint_host, remaining_connect_timeout(connect_deadline)?).await?;
    let room_bare = BareJid::from_str(WXWIRE_ROOM)
        .map_err(|err| WxWireTransportError::InvalidRoomJid(err.to_string()))?;

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
        .send_raw(
            "<starttls xmlns='urn:ietf:params:xml:ns:xmpp-tls'/>",
            write_timeout,
        )
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
    session.send_raw(auth.as_str(), write_timeout).await?;

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
    session.send_raw(bind_iq.as_str(), write_timeout).await?;

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
            .send_raw(
                "<enable xmlns='urn:xmpp:sm:3' resume='true'/>",
                write_timeout,
            )
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
    session.send_raw(join.as_str(), write_timeout).await?;

    let join_confirm = tokio::time::timeout(remaining_connect_timeout(connect_deadline)?, async {
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

    let (socket, read_buf) = session.into_parts();
    Ok(ConnectedSession {
        socket,
        room_bare,
        label: format!("{endpoint_host}:{WXWIRE_PORT} room={WXWIRE_ROOM}"),
        read_buf,
        sm_enabled,
    })
}

#[derive(Debug)]
pub(super) enum XmppSocket {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl XmppSocket {
    pub(super) async fn read_some(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.read(buf).await,
            Self::Tls(stream) => stream.read(buf).await,
        }
    }

    pub(super) async fn write_all(
        &mut self,
        bytes: &[u8],
        timeout: Duration,
    ) -> WxWireReceiverResult<()> {
        match self {
            Self::Plain(stream) => write_all_with_timeout(stream, bytes, timeout).await,
            Self::Tls(stream) => write_all_with_timeout(stream, bytes, timeout).await,
        }
    }

    pub(super) async fn shutdown(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(stream) => stream.shutdown().await,
            Self::Tls(stream) => stream.shutdown().await,
        }
    }
}

#[derive(Debug)]
pub(super) struct XmppSession {
    endpoint_host: String,
    socket: Option<XmppSocket>,
    read_buf: String,
}

impl XmppSession {
    pub(super) async fn connect(
        endpoint_host: &str,
        timeout: Duration,
    ) -> WxWireReceiverResult<Self> {
        let addr = format!("{endpoint_host}:{WXWIRE_PORT}");
        let tcp = tokio::time::timeout(timeout, TcpStream::connect(addr.as_str()))
            .await
            .map_err(|_| WxWireTransportError::ConnectTimeout)
            .and_then(|result| {
                result.map_err(|err| WxWireTransportError::TcpConnect(err.to_string()))
            })?;
        Ok(Self::new(endpoint_host.to_string(), XmppSocket::Plain(tcp)))
    }

    pub(super) fn new(endpoint_host: String, socket: XmppSocket) -> Self {
        Self {
            endpoint_host,
            socket: Some(socket),
            read_buf: String::new(),
        }
    }

    pub(super) fn into_parts(self) -> (Option<XmppSocket>, String) {
        (self.socket, self.read_buf)
    }

    pub(super) async fn send_raw(
        &mut self,
        xml: &str,
        timeout: Duration,
    ) -> WxWireReceiverResult<()> {
        self.socket
            .as_mut()
            .ok_or(WxWireTransportError::SocketNotAvailable)?
            .write_all(xml.as_bytes(), timeout)
            .await
    }

    pub(super) async fn open_stream(&mut self, timeout: Duration) -> WxWireReceiverResult<()> {
        let open = format!(
            "<?xml version='1.0' encoding='utf-8'?><stream:stream xmlns='jabber:client' xmlns:stream='http://etherx.jabber.org/streams' to='{}' version='1.0'>",
            self.endpoint_host
        );
        self.send_raw(open.as_str(), timeout).await
    }

    pub(super) async fn upgrade_tls(&mut self, timeout: Duration) -> WxWireReceiverResult<()> {
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
        let connector = TlsConnector::from(Arc::new(config));
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

    pub(super) async fn wait_for_tag(
        &mut self,
        tag: &str,
        timeout: Duration,
    ) -> WxWireReceiverResult<String> {
        self.wait_for_any_tag(&[tag], timeout).await
    }

    pub(super) async fn wait_for_any_tag(
        &mut self,
        tags: &[&str],
        timeout: Duration,
    ) -> WxWireReceiverResult<String> {
        let wait_label = tags.join(" or ");
        loop {
            while let Some(elem) = super::stanza::pop_next_top_level_element(&mut self.read_buf) {
                if tags.iter().any(|tag| {
                    super::stanza::stanza_root_tag_name(elem.as_str())
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

async fn write_all_with_timeout<W>(
    writer: &mut W,
    bytes: &[u8],
    timeout: Duration,
) -> WxWireReceiverResult<()>
where
    W: AsyncWrite + Unpin,
{
    tokio::time::timeout(timeout, writer.write_all(bytes))
        .await
        .map_err(|_| WxWireTransportError::WriteTimeout)?
        .map_err(|err| WxWireTransportError::WriteFailed(err.to_string()).into())
}

pub(super) fn chrono_like_suffix() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", now.as_secs())
}

pub(super) fn remaining_connect_timeout(deadline: Instant) -> WxWireReceiverResult<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(|| WxWireTransportError::ConnectTimeout.into())
}

pub(super) fn append_with_read_limit(
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

#[cfg(test)]
mod tests {
    use super::write_all_with_timeout;
    use crate::wxwire_receiver::error::{WxWireReceiverError, WxWireTransportError};
    use std::time::Duration;

    #[tokio::test]
    async fn write_all_with_timeout_returns_timeout_error() {
        let (mut writer, _reader) = tokio::io::duplex(1);
        let payload = vec![b'x'; 1024];

        let error = write_all_with_timeout(&mut writer, &payload, Duration::from_millis(10))
            .await
            .expect_err("write should time out");

        assert!(matches!(
            error,
            WxWireReceiverError::Transport(WxWireTransportError::WriteTimeout)
        ));
    }
}
