//! Small helpers shared across live CLI modes.

use crate::default_servers::default_upstream_servers;
use crate::error::{LiveError, LiveResult};
use emwin_protocol::qbt_receiver::parse_qbt_server;

/// Parses `--server` values or falls back to the default upstream list.
pub(crate) fn parse_servers_or_default(raw_servers: &[String]) -> LiveResult<Vec<(String, u16)>> {
    if raw_servers.is_empty() {
        return Ok(default_upstream_servers());
    }

    raw_servers
        .iter()
        .map(|entry| {
            parse_qbt_server(entry).ok_or_else(|| {
                LiveError::invalid_argument(format!(
                    "invalid --server entry: {entry} (expected host:port)"
                ))
            })
        })
        .collect()
}
