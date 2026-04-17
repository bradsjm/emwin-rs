//! Wire-level server-list helpers for EMWIN protocol.

use bytes::Bytes;

use crate::qbt_receiver::protocol::auth::xor_ff;

const DEFAULT_SCAN_BUFFER_BYTES: usize = 65_536;

/// Stateful scanner that extracts XOR-encoded `/ServerList/...` frames from wire chunks.
#[derive(Debug)]
pub struct QbtServerListWireScanner {
    decoded_buffer: Vec<u8>,
    max_buffer_bytes: usize,
}

impl Default for QbtServerListWireScanner {
    fn default() -> Self {
        Self::new(DEFAULT_SCAN_BUFFER_BYTES)
    }
}

impl QbtServerListWireScanner {
    /// Creates a scanner with the requested internal buffer budget.
    pub fn new(max_buffer_bytes: usize) -> Self {
        Self {
            decoded_buffer: Vec::new(),
            max_buffer_bytes: max_buffer_bytes.max(1024),
        }
    }

    /// Observes a wire chunk and returns the latest complete server-list frame, if present.
    ///
    /// Returned bytes are XOR-encoded exactly as they should appear on the wire.
    pub fn observe_wire_chunk(&mut self, wire: &[u8]) -> Option<Bytes> {
        self.decoded_buffer
            .extend(wire.iter().map(|byte| byte ^ 0xFF));
        if self.decoded_buffer.len() > self.max_buffer_bytes {
            let drop_count = self.decoded_buffer.len() - self.max_buffer_bytes;
            self.decoded_buffer.drain(..drop_count);
        }

        let prefix = b"/ServerList/";
        let mut latest = None;
        while let Some(start) = find_subsequence(&self.decoded_buffer, prefix) {
            let slice = &self.decoded_buffer[start..];
            let Some(end_rel) = slice.iter().position(|byte| *byte == 0) else {
                break;
            };
            let end = start + end_rel + 1;
            let frame = Bytes::copy_from_slice(&self.decoded_buffer[start..end]);
            latest = Some(xor_ff(&frame));
            self.decoded_buffer.drain(..end);
        }

        latest
    }
}

/// Builds an XOR-encoded `/ServerList/...` frame from server endpoints.
pub fn build_server_list_wire(servers: &[(String, u16)]) -> Bytes {
    let entries = servers
        .iter()
        .map(|(host, port)| format!("{host}:{port}"))
        .collect::<Vec<_>>()
        .join("|");
    let payload = format!("/ServerList/{entries}\0");
    xor_ff(payload.as_bytes())
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::{QbtServerListWireScanner, build_server_list_wire};

    #[test]
    fn build_server_list_wire_formats_expected_payload() {
        let wire = build_server_list_wire(&[
            ("a.example".to_string(), 2211),
            ("b.example".to_string(), 1000),
        ]);
        let decoded = wire.iter().map(|byte| byte ^ 0xFF).collect::<Vec<_>>();
        assert_eq!(
            decoded,
            b"/ServerList/a.example:2211|b.example:1000\0".to_vec()
        );
    }

    #[test]
    fn scanner_extracts_latest_complete_server_list_frame() {
        let first = build_server_list_wire(&[("first.example".to_string(), 2211)]);
        let second = build_server_list_wire(&[("second.example".to_string(), 1000)]);
        let mut scanner = QbtServerListWireScanner::default();

        let mut chunk = Vec::new();
        chunk.extend_from_slice(&first);
        chunk.extend_from_slice(&second[..second.len() / 2]);

        let first_result = scanner.observe_wire_chunk(&chunk);
        assert_eq!(first_result.as_deref(), Some(first.as_ref()));

        let second_result = scanner.observe_wire_chunk(&second[second.len() / 2..]);
        assert_eq!(second_result.as_deref(), Some(second.as_ref()));
    }
}
