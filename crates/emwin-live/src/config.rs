//! Build receiver configuration for live CLI commands.
//!
//! This module keeps CLI-facing argument validation separate from the protocol crate's config
//! types so command handlers can stay focused on orchestration.

#![allow(missing_docs)]

use crate::error::{LiveError, LiveResult};
use crate::shared::parse_servers_or_default;
use emwin_protocol::qbt_receiver::{QbtDecodeConfig, QbtReceiverConfig};
use emwin_protocol::wxwire_receiver::WxWireReceiverConfig;
use std::path::PathBuf;

/// Normalized inputs used to build a live receiver configuration.
pub(crate) struct LiveConfigRequest {
    pub username: Option<String>,
    pub password: Option<String>,
    pub raw_servers: Vec<String>,
    pub server_list_path: Option<String>,
    pub idle_timeout_secs: u64,
    pub qbt_watchdog_timeout_secs: u64,
    pub username_context: &'static str,
    pub password_context: &'static str,
}

pub(crate) fn build_qbt_receiver_config(
    request: LiveConfigRequest,
) -> LiveResult<QbtReceiverConfig> {
    let LiveConfigRequest {
        username,
        password,
        raw_servers,
        server_list_path,
        qbt_watchdog_timeout_secs,
        username_context,
        ..
    } = request;

    if password.is_some() {
        return Err(LiveError::invalid_argument(
            "--password is not supported with --receiver qbt",
        ));
    }

    let username = username.ok_or_else(|| {
        LiveError::invalid_argument(format!("{username_context} requires --username"))
    })?;
    let pin_servers = !raw_servers.is_empty();
    if pin_servers && server_list_path.is_some() {
        return Err(LiveError::invalid_argument(
            "--server-list-path is not supported when --server pins the QBT server list",
        ));
    }
    let servers = parse_servers_or_default(&raw_servers)?;

    Ok(QbtReceiverConfig {
        email: username,
        servers,
        server_list_path: server_list_path.map(PathBuf::from),
        follow_server_list_updates: !pin_servers,
        reconnect_delay_secs: 5,
        connection_timeout_secs: 5,
        write_timeout_secs: 10,
        watchdog_timeout_secs: qbt_watchdog_timeout_secs,
        max_exceptions: 10,
        decode: QbtDecodeConfig::default(),
    })
}

pub(crate) fn build_wxwire_receiver_config(
    request: LiveConfigRequest,
) -> LiveResult<WxWireReceiverConfig> {
    let LiveConfigRequest {
        username,
        password,
        raw_servers,
        server_list_path,
        idle_timeout_secs,
        username_context,
        password_context,
        ..
    } = request;

    if !raw_servers.is_empty() {
        return Err(LiveError::invalid_argument(
            "--server is not supported with --receiver wxwire",
        ));
    }
    if server_list_path.is_some() {
        return Err(LiveError::invalid_argument(
            "--server-list-path is not supported with --receiver wxwire",
        ));
    }

    let username = username.ok_or_else(|| {
        LiveError::invalid_argument(format!("{username_context} requires --username"))
    })?;
    let password = password.ok_or_else(|| {
        LiveError::invalid_argument(format!("{password_context} requires --password"))
    })?;

    Ok(WxWireReceiverConfig {
        username,
        password,
        idle_timeout_secs: idle_timeout_secs.max(1),
        write_timeout_secs: 10,
        ..WxWireReceiverConfig::default()
    })
}

#[cfg(test)]
mod tests {
    use super::{LiveConfigRequest, build_qbt_receiver_config};

    fn qbt_request() -> LiveConfigRequest {
        LiveConfigRequest {
            username: Some("user@example.com".to_string()),
            password: None,
            raw_servers: Vec::new(),
            server_list_path: None,
            idle_timeout_secs: 90,
            qbt_watchdog_timeout_secs: 20,
            username_context: "test",
            password_context: "test",
        }
    }

    #[test]
    fn qbt_pinned_servers_reject_server_list_path() {
        let mut request = qbt_request();
        request.raw_servers = vec!["127.0.0.1:2211".to_string()];
        request.server_list_path = Some("/tmp/emwin-servers.json".to_string());

        match build_qbt_receiver_config(request) {
            Ok(_) => panic!("config should reject combo"),
            Err(err) => assert!(
                err.to_string().contains("--server-list-path"),
                "unexpected error: {err}"
            ),
        }
    }

    #[test]
    fn qbt_pinned_servers_disable_automatic_server_list_behavior() {
        let mut request = qbt_request();
        request.raw_servers = vec!["example.com:2211".to_string()];

        let config = build_qbt_receiver_config(request).expect("config should build");

        assert!(!config.follow_server_list_updates);
        assert!(config.server_list_path.is_none());
        assert_eq!(config.servers, vec![("example.com".to_string(), 2211)]);
    }

    #[test]
    fn qbt_default_servers_keep_automatic_server_list_mode() {
        let mut request = qbt_request();
        request.server_list_path = Some("/tmp/emwin-servers.json".to_string());

        let config = build_qbt_receiver_config(request).expect("request must succeed");

        assert!(config.follow_server_list_updates);
        assert!(config.server_list_path.is_some());
    }
}
