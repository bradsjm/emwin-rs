//! Build and parse QBT authentication messages.

use bytes::Bytes;

use crate::qbt_receiver::protocol::model::QbtAuthMessage;

/// Interval between re-authentication messages in seconds.
///
/// The client must refresh logon state before the server's idle window expires.
pub const REAUTH_INTERVAL_SECS: u64 = 115;
pub const LOGON_PREFIX: &str = "ByteBlast Client|NM-";
pub const LOGON_SUFFIX: &str = "|V2";

/// Builds the wire-format logon message for a user email address.
pub fn build_logon_message(email: &str) -> String {
    format!("{LOGON_PREFIX}{email}{LOGON_SUFFIX}")
}

/// Parses a logon message and extracts the authentication payload.
pub fn parse_logon_message(message: &str) -> Option<QbtAuthMessage> {
    let payload = message.strip_prefix(LOGON_PREFIX)?;
    let email = payload.strip_suffix(LOGON_SUFFIX)?.trim();
    if email.is_empty() {
        return None;
    }
    Some(QbtAuthMessage {
        email: email.to_string(),
    })
}

/// Applies the protocol's XOR-`0xFF` wire transform.
pub fn xor_ff(data: &[u8]) -> Bytes {
    let encoded: Vec<u8> = data.iter().map(|b| b ^ 0xFF).collect();
    Bytes::from(encoded)
}

#[cfg(test)]
mod tests {
    use super::parse_logon_message;

    #[test]
    fn parse_logon_message_extracts_email() {
        let parsed =
            parse_logon_message("ByteBlast Client|NM-user@example.com|V2").expect("valid logon");
        assert_eq!(parsed.email, "user@example.com");
    }

    #[test]
    fn parse_logon_message_rejects_invalid_shapes() {
        assert!(parse_logon_message("ByteBlast Client|NM-|V2").is_none());
        assert!(parse_logon_message("ByteBlast Client|NM-user@example.com|V1").is_none());
    }
}
