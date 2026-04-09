use crate::wxwire_receiver::error::{WxWireReceiverResult, WxWireTransportError};
use minidom::Element;
use quick_xml::Reader;
use quick_xml::events::Event as XmlEvent;
use xmpp_parsers::jid::BareJid;

const CLIENT_NS: &str = "jabber:client";

pub(super) fn pop_next_top_level_element(buf: &mut String) -> Option<String> {
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

pub(super) fn stanza_root_tag_name(stanza: &str) -> Option<String> {
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

pub(super) fn is_supported_top_level_stanza(stanza: &str) -> bool {
    matches!(
        stanza_root_tag_name(stanza).as_deref(),
        Some("message" | "presence" | "iq" | "r" | "a")
    )
}

pub(super) fn is_room_join_presence(
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

pub(super) fn parse_element_with_default_ns(xml: &str) -> Result<Element, minidom::Error> {
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
        add_default_client_ns, is_room_join_presence, is_supported_top_level_stanza,
        pop_next_top_level_element, stanza_root_tag_name,
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
        let err =
            super::super::session::append_with_read_limit(&mut buf, "12345", "buffer too large")
                .expect_err("chunk should exceed max read buffer size");

        assert!(matches!(
            err,
            WxWireReceiverError::Transport(WxWireTransportError::BufferOverflow(message))
            if message == "buffer too large"
        ));
        assert!(buf.is_empty());
    }
}
