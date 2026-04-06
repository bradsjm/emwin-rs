//! Build receiver configuration for live CLI commands.
//!
//! This module keeps CLI-facing argument validation separate from the protocol crate's config
//! types so command handlers can stay focused on orchestration.

use crate::ReceiverKind;
use crate::live::shared::parse_servers_or_default;
use emwin_protocol::qbt_receiver::{QbtDecodeConfig, QbtReceiverConfig};
use emwin_protocol::wxwire_receiver::WxWireReceiverConfig;
use std::path::PathBuf;

/// Receiver-specific configuration produced from CLI live-mode arguments.
pub(crate) enum LiveReceiverConfig {
    Qbt(QbtReceiverConfig),
    WxWire(WxWireReceiverConfig),
}

/// Normalized inputs used to build a live receiver configuration.
pub(crate) struct LiveConfigRequest {
    pub receiver: ReceiverKind,
    pub username: Option<String>,
    pub password: Option<String>,
    pub raw_servers: Vec<String>,
    pub server_list_path: Option<String>,
    pub idle_timeout_secs: u64,
    pub qbt_watchdog_timeout_secs: u64,
    pub username_context: &'static str,
    pub password_context: &'static str,
}

/// Builds the concrete receiver configuration requested by the CLI.
pub(crate) fn build_live_receiver_config(
    request: LiveConfigRequest,
) -> crate::error::CliResult<LiveReceiverConfig> {
    match request.receiver {
        ReceiverKind::Qbt => build_qbt_receiver_config(request).map(LiveReceiverConfig::Qbt),
        ReceiverKind::Wxwire => {
            build_wxwire_receiver_config(request).map(LiveReceiverConfig::WxWire)
        }
    }
}

fn build_qbt_receiver_config(
    request: LiveConfigRequest,
) -> crate::error::CliResult<QbtReceiverConfig> {
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
        return Err(crate::error::CliError::invalid_argument(
            "--password is not supported with --receiver qbt",
        ));
    }

    let username = username.ok_or_else(|| {
        crate::error::CliError::invalid_argument(format!("{username_context} requires --username"))
    })?;
    let pin_servers = !raw_servers.is_empty();
    let servers = parse_servers_or_default(&raw_servers)?;

    Ok(QbtReceiverConfig {
        email: username,
        servers,
        server_list_path: server_list_path.map(PathBuf::from),
        follow_server_list_updates: !pin_servers,
        reconnect_delay_secs: 5,
        connection_timeout_secs: 5,
        watchdog_timeout_secs: qbt_watchdog_timeout_secs,
        max_exceptions: 10,
        decode: QbtDecodeConfig::default(),
    })
}

fn build_wxwire_receiver_config(
    request: LiveConfigRequest,
) -> crate::error::CliResult<WxWireReceiverConfig> {
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
        return Err(crate::error::CliError::invalid_argument(
            "--server is not supported with --receiver wxwire",
        ));
    }
    if server_list_path.is_some() {
        return Err(crate::error::CliError::invalid_argument(
            "--server-list-path is not supported with --receiver wxwire",
        ));
    }

    let username = username.ok_or_else(|| {
        crate::error::CliError::invalid_argument(format!("{username_context} requires --username"))
    })?;
    let password = password.ok_or_else(|| {
        crate::error::CliError::invalid_argument(format!("{password_context} requires --password"))
    })?;

    Ok(WxWireReceiverConfig {
        username,
        password,
        idle_timeout_secs: idle_timeout_secs.max(1),
        ..WxWireReceiverConfig::default()
    })
}
