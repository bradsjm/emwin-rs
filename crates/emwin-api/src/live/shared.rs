//! Small helpers shared across live CLI modes.

use crate::default_servers::default_upstream_servers;
use emwin_protocol::qbt_receiver::parse_qbt_server;
use std::time::{SystemTime, UNIX_EPOCH};

/// Converts a system time into Unix seconds, returning `0` for pre-epoch values.
pub(crate) fn unix_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Parses `--server` values or falls back to the default upstream list.
pub(crate) fn parse_servers_or_default(
    raw_servers: &[String],
) -> crate::error::CliResult<Vec<(String, u16)>> {
    if raw_servers.is_empty() {
        return Ok(default_upstream_servers());
    }

    raw_servers
        .iter()
        .map(|entry| {
            parse_qbt_server(entry).ok_or_else(|| {
                crate::error::CliError::invalid_argument(format!(
                    "invalid --server entry: {entry} (expected host:port)"
                ))
            })
        })
        .collect()
}
