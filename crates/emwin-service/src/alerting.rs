use crate::FileFilterInput;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Durable alert source kinds accepted by the alerting subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertSourceKind {
    ProductAvailable,
    IncidentChange,
}

/// Contact-point transport kinds supported by V1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertContactPointKind {
    Webhook,
    Apprise,
}

/// Stable delivery status persisted for one target attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertDeliveryStatus {
    Pending,
    InProgress,
    Delivered,
    RetryPending,
    Failed,
    Suppressed,
}

/// Flat incident-match criteria shared across alerting and HTTP consumers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncidentFilterInput {
    pub action: Option<String>,
    pub office: Option<String>,
    pub phenomena: Option<String>,
    pub significance: Option<String>,
    pub status: Option<String>,
    pub etn: Option<String>,
}

/// Canonical stored criteria shape for one alert rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source_kind", rename_all = "snake_case")]
pub enum AlertMatchCriteria {
    ProductAvailable(Box<FileFilterInput>),
    IncidentChange(Box<IncidentFilterInput>),
}

/// Runtime behavior attached to a rule match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertTriggerPolicy {
    pub cooldown_secs: u64,
    pub severity: Option<String>,
}

/// Stored title/body templates rendered by the worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertTemplate {
    pub title: String,
    pub body: String,
}

/// Secret-bearing contact-point configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AlertContactPointConfig {
    Webhook {
        url: String,
        authorization_header: Option<String>,
        signing_secret: Option<String>,
        timeout_secs: Option<u64>,
    },
    Apprise {
        destination_url: String,
    },
}

/// Redacted contact-point configuration safe for API reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AlertContactPointConfigView {
    Webhook {
        url: String,
        has_authorization_header: bool,
        has_signing_secret: bool,
        timeout_secs: Option<u64>,
    },
    Apprise {
        has_destination_url: bool,
    },
}

impl AlertContactPointConfig {
    pub fn kind(&self) -> AlertContactPointKind {
        match self {
            Self::Webhook { .. } => AlertContactPointKind::Webhook,
            Self::Apprise { .. } => AlertContactPointKind::Apprise,
        }
    }

    pub fn redact(&self) -> AlertContactPointConfigView {
        match self {
            Self::Webhook {
                url,
                authorization_header,
                signing_secret,
                timeout_secs,
            } => AlertContactPointConfigView::Webhook {
                url: url.clone(),
                has_authorization_header: authorization_header.is_some(),
                has_signing_secret: signing_secret.is_some(),
                timeout_secs: *timeout_secs,
            },
            Self::Apprise { destination_url } => AlertContactPointConfigView::Apprise {
                has_destination_url: !destination_url.trim().is_empty(),
            },
        }
    }
}

/// Stored destination definition returned by alerting APIs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertContactPoint {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub kind: AlertContactPointKind,
    pub config: AlertContactPointConfigView,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Contact-point create/update payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertContactPointInput {
    pub name: String,
    pub enabled: bool,
    pub config: AlertContactPointConfig,
}

/// One ordered target binding attached to a rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertRuleTarget {
    pub contact_point_id: i64,
    pub position: i32,
}

/// Persisted alert rule returned by alerting APIs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub criteria: AlertMatchCriteria,
    pub trigger_policy: AlertTriggerPolicy,
    pub template: AlertTemplate,
    pub targets: Vec<AlertRuleTarget>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Rule create/update payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertRuleInput {
    pub name: String,
    pub enabled: bool,
    pub criteria: AlertMatchCriteria,
    pub trigger_policy: AlertTriggerPolicy,
    pub template: AlertTemplate,
    pub targets: Vec<AlertRuleTarget>,
}

/// Temporary notification suppression record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertSilence {
    pub id: i64,
    pub criteria: AlertMatchCriteria,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub reason: String,
    pub created_at: DateTime<Utc>,
}

/// Silence create payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertSilenceInput {
    pub criteria: AlertMatchCriteria,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub reason: String,
}

/// Durable source event written from archive persistence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertSourceEvent {
    pub id: i64,
    pub source_kind: AlertSourceKind,
    pub source_id: String,
    pub payload: Value,
    pub source_timestamp: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub processed_at: Option<DateTime<Utc>>,
}

/// Logical alert firing kept for audit and per-target fanout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertEvent {
    pub id: i64,
    pub rule_id: i64,
    pub source_event_id: i64,
    pub delivery_key: String,
    pub severity: Option<String>,
    pub title: String,
    pub body: String,
    pub payload: Value,
    pub created_at: DateTime<Utc>,
}

/// One persisted delivery attempt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertDeliveryAttempt {
    pub id: i64,
    pub alert_event_id: i64,
    pub contact_point_id: i64,
    pub delivery_key: String,
    pub attempt_no: i32,
    pub status: AlertDeliveryStatus,
    pub claimed_at: Option<DateTime<Utc>>,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub response_code: Option<i32>,
    pub response_excerpt: Option<String>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Simulation request for draft or persisted rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertSimulationRequest {
    pub criteria: AlertMatchCriteria,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub sample_limit: usize,
}

/// Sample source event returned from rule simulation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertSimulationSample {
    pub source_event_id: i64,
    pub source_kind: AlertSourceKind,
    pub source_id: String,
    pub source_timestamp: DateTime<Utc>,
    pub payload: Value,
}

/// Summary returned from rule simulation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertSimulationResult {
    pub total_matches: usize,
    pub samples: Vec<AlertSimulationSample>,
}
