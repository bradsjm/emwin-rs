//! Configuration types for Weather Wire receiver.
//!
//! This module defines configuration structures for the Weather Wire XMPP receiver
//! and provides validation for required fields.

#![allow(missing_docs)]

use crate::wxwire_receiver::error::{WxWireConfigError, WxWireReceiverError};

/// Primary NWWS-OI endpoint hostname.
pub const WXWIRE_PRIMARY_HOST: &str = "nwws-oi.weather.gov";
/// NWWS-OI XMPP port.
pub const WXWIRE_PORT: u16 = 5222;
/// Fixed MUC room name.
pub const WXWIRE_ROOM: &str = "nwws@conference.nwws-oi.weather.gov";

/// Runtime configuration for Weather Wire.
#[derive(Clone)]
pub struct WxWireReceiverConfig {
    /// NWWS-OI username.
    pub username: String,
    /// NWWS-OI password.
    pub password: String,
    /// Idle timeout window for stalled-connection warnings in seconds.
    pub idle_timeout_secs: u64,
    /// Capacity of the event channel.
    pub event_channel_capacity: usize,
    /// Capacity of the inbound stanza channel.
    pub inbound_channel_capacity: usize,
    /// Interval for telemetry emission in seconds.
    pub telemetry_emit_interval_secs: u64,
    /// Timeout for establishing the XMPP connection and session.
    pub connect_timeout_secs: u64,
    /// Timeout for outbound socket writes.
    pub write_timeout_secs: u64,
}

impl std::fmt::Debug for WxWireReceiverConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WxWireReceiverConfig")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("idle_timeout_secs", &self.idle_timeout_secs)
            .field("event_channel_capacity", &self.event_channel_capacity)
            .field("inbound_channel_capacity", &self.inbound_channel_capacity)
            .field(
                "telemetry_emit_interval_secs",
                &self.telemetry_emit_interval_secs,
            )
            .field("connect_timeout_secs", &self.connect_timeout_secs)
            .field("write_timeout_secs", &self.write_timeout_secs)
            .finish()
    }
}

impl Default for WxWireReceiverConfig {
    fn default() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            idle_timeout_secs: 90,
            event_channel_capacity: 1024,
            inbound_channel_capacity: 512,
            telemetry_emit_interval_secs: 5,
            connect_timeout_secs: 10,
            write_timeout_secs: 10,
        }
    }
}

impl WxWireReceiverConfig {
    /// Validates configuration fields.
    pub fn validate(&self) -> Result<(), WxWireReceiverError> {
        if self.username.trim().is_empty() {
            return Err(WxWireConfigError::EmptyUsername.into());
        }

        if self.password.trim().is_empty() {
            return Err(WxWireConfigError::EmptyPassword.into());
        }

        if self.idle_timeout_secs == 0 {
            return Err(WxWireConfigError::ZeroIdleTimeout.into());
        }

        if self.event_channel_capacity == 0 {
            return Err(WxWireConfigError::ZeroEventChannelCapacity.into());
        }

        if self.inbound_channel_capacity == 0 {
            return Err(WxWireConfigError::ZeroInboundChannelCapacity.into());
        }

        if self.telemetry_emit_interval_secs == 0 {
            return Err(WxWireConfigError::ZeroTelemetryEmitInterval.into());
        }
        if self.connect_timeout_secs == 0 {
            return Err(WxWireConfigError::ZeroConnectTimeout.into());
        }
        if self.write_timeout_secs == 0 {
            return Err(WxWireConfigError::ZeroWriteTimeout.into());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::WxWireReceiverConfig;

    #[test]
    fn validate_rejects_missing_credentials() {
        let cfg = WxWireReceiverConfig::default();
        assert!(cfg.validate().is_err());

        let cfg = WxWireReceiverConfig {
            username: "user".to_string(),
            ..WxWireReceiverConfig::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_accepts_valid_config() {
        let cfg = WxWireReceiverConfig {
            username: "user".to_string(),
            password: "pass".to_string(),
            ..WxWireReceiverConfig::default()
        };

        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn debug_redacts_password() {
        let cfg = WxWireReceiverConfig {
            username: "user".to_string(),
            password: "super-secret".to_string(),
            ..WxWireReceiverConfig::default()
        };

        let debug = format!("{cfg:?}");
        assert!(debug.contains("username"));
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("super-secret"));
    }

    #[test]
    fn validate_rejects_zero_write_timeout() {
        let cfg = WxWireReceiverConfig {
            username: "user".to_string(),
            password: "pass".to_string(),
            write_timeout_secs: 0,
            ..WxWireReceiverConfig::default()
        };

        assert!(cfg.validate().is_err());
    }
}
