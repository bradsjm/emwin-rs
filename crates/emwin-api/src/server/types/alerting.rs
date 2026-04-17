use emwin_service::{
    AlertContactPoint, AlertContactPointConfig, AlertContactPointInput, AlertDeliveryAttempt,
    AlertEvent, AlertMatchCriteria, AlertRule, AlertRuleInput, AlertRuleTarget, AlertSilence,
    AlertSilenceInput, AlertSimulationRequest, AlertSimulationResult, AlertSimulationSample,
    AlertTemplate, AlertTriggerPolicy,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct AlertContactPointPayload {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) enabled: bool,
    pub(crate) kind: String,
    #[schema(value_type = Object)]
    pub(crate) config: serde_json::Value,
    pub(crate) created_at: chrono::DateTime<chrono::Utc>,
    pub(crate) updated_at: chrono::DateTime<chrono::Utc>,
}

impl AlertContactPointPayload {
    pub(crate) fn from_contact_point(contact_point: AlertContactPoint) -> serde_json::Result<Self> {
        Ok(Self {
            id: contact_point.id,
            name: contact_point.name,
            enabled: contact_point.enabled,
            kind: serde_json::to_value(contact_point.kind)?
                .as_str()
                .unwrap_or("unknown")
                .to_string(),
            config: serde_json::to_value(contact_point.config)?,
            created_at: contact_point.created_at,
            updated_at: contact_point.updated_at,
        })
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct AlertContactPointInputPayload {
    pub(crate) name: String,
    pub(crate) enabled: bool,
    #[schema(value_type = Object)]
    pub(crate) config: serde_json::Value,
}

impl AlertContactPointInputPayload {
    pub(crate) fn into_domain(self) -> serde_json::Result<AlertContactPointInput> {
        Ok(AlertContactPointInput {
            name: self.name,
            enabled: self.enabled,
            config: serde_json::from_value::<AlertContactPointConfig>(self.config)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub(crate) struct AlertRuleTargetPayload {
    pub(crate) contact_point_id: i64,
    pub(crate) position: i32,
}

impl From<AlertRuleTarget> for AlertRuleTargetPayload {
    fn from(value: AlertRuleTarget) -> Self {
        Self {
            contact_point_id: value.contact_point_id,
            position: value.position,
        }
    }
}

impl From<AlertRuleTargetPayload> for AlertRuleTarget {
    fn from(value: AlertRuleTargetPayload) -> Self {
        Self {
            contact_point_id: value.contact_point_id,
            position: value.position,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct AlertRulePayload {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) enabled: bool,
    #[schema(value_type = Object)]
    pub(crate) criteria: serde_json::Value,
    #[schema(value_type = Object)]
    pub(crate) trigger_policy: serde_json::Value,
    #[schema(value_type = Object)]
    pub(crate) template: serde_json::Value,
    pub(crate) targets: Vec<AlertRuleTargetPayload>,
    pub(crate) created_at: chrono::DateTime<chrono::Utc>,
    pub(crate) updated_at: chrono::DateTime<chrono::Utc>,
}

impl AlertRulePayload {
    pub(crate) fn from_rule(rule: AlertRule) -> serde_json::Result<Self> {
        Ok(Self {
            id: rule.id,
            name: rule.name,
            enabled: rule.enabled,
            criteria: serde_json::to_value(rule.criteria)?,
            trigger_policy: serde_json::to_value(rule.trigger_policy)?,
            template: serde_json::to_value(rule.template)?,
            targets: rule.targets.into_iter().map(Into::into).collect(),
            created_at: rule.created_at,
            updated_at: rule.updated_at,
        })
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct AlertRuleInputPayload {
    pub(crate) name: String,
    pub(crate) enabled: bool,
    #[schema(value_type = Object)]
    pub(crate) criteria: serde_json::Value,
    #[schema(value_type = Object)]
    pub(crate) trigger_policy: serde_json::Value,
    #[schema(value_type = Object)]
    pub(crate) template: serde_json::Value,
    pub(crate) targets: Vec<AlertRuleTargetPayload>,
}

impl AlertRuleInputPayload {
    pub(crate) fn into_domain(self) -> serde_json::Result<AlertRuleInput> {
        Ok(AlertRuleInput {
            name: self.name,
            enabled: self.enabled,
            criteria: serde_json::from_value::<AlertMatchCriteria>(self.criteria)?,
            trigger_policy: serde_json::from_value::<AlertTriggerPolicy>(self.trigger_policy)?,
            template: serde_json::from_value::<AlertTemplate>(self.template)?,
            targets: self.targets.into_iter().map(Into::into).collect(),
        })
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct AlertSilencePayload {
    pub(crate) id: i64,
    #[schema(value_type = Object)]
    pub(crate) criteria: serde_json::Value,
    pub(crate) starts_at: chrono::DateTime<chrono::Utc>,
    pub(crate) ends_at: chrono::DateTime<chrono::Utc>,
    pub(crate) reason: String,
    pub(crate) created_at: chrono::DateTime<chrono::Utc>,
}

impl AlertSilencePayload {
    pub(crate) fn from_silence(silence: AlertSilence) -> serde_json::Result<Self> {
        Ok(Self {
            id: silence.id,
            criteria: serde_json::to_value(silence.criteria)?,
            starts_at: silence.starts_at,
            ends_at: silence.ends_at,
            reason: silence.reason,
            created_at: silence.created_at,
        })
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct AlertSilenceInputPayload {
    #[schema(value_type = Object)]
    pub(crate) criteria: serde_json::Value,
    pub(crate) starts_at: chrono::DateTime<chrono::Utc>,
    pub(crate) ends_at: chrono::DateTime<chrono::Utc>,
    pub(crate) reason: String,
}

impl AlertSilenceInputPayload {
    pub(crate) fn into_domain(self) -> serde_json::Result<AlertSilenceInput> {
        Ok(AlertSilenceInput {
            criteria: serde_json::from_value::<AlertMatchCriteria>(self.criteria)?,
            starts_at: self.starts_at,
            ends_at: self.ends_at,
            reason: self.reason,
        })
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct AlertEventPayload {
    pub(crate) id: i64,
    pub(crate) rule_id: i64,
    pub(crate) source_event_id: i64,
    pub(crate) delivery_key: String,
    pub(crate) severity: Option<String>,
    pub(crate) title: String,
    pub(crate) body: String,
    #[schema(value_type = Object)]
    pub(crate) payload: serde_json::Value,
    pub(crate) created_at: chrono::DateTime<chrono::Utc>,
}

impl From<AlertEvent> for AlertEventPayload {
    fn from(value: AlertEvent) -> Self {
        Self {
            id: value.id,
            rule_id: value.rule_id,
            source_event_id: value.source_event_id,
            delivery_key: value.delivery_key,
            severity: value.severity,
            title: value.title,
            body: value.body,
            payload: value.payload,
            created_at: value.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct AlertDeliveryAttemptPayload {
    pub(crate) id: i64,
    pub(crate) alert_event_id: i64,
    pub(crate) contact_point_id: i64,
    pub(crate) delivery_key: String,
    pub(crate) attempt_no: i32,
    pub(crate) status: String,
    pub(crate) next_retry_at: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) response_code: Option<i32>,
    pub(crate) response_excerpt: Option<String>,
    pub(crate) delivered_at: Option<chrono::DateTime<chrono::Utc>>,
    pub(crate) created_at: chrono::DateTime<chrono::Utc>,
    pub(crate) updated_at: chrono::DateTime<chrono::Utc>,
}

impl AlertDeliveryAttemptPayload {
    pub(crate) fn from_attempt(attempt: AlertDeliveryAttempt) -> serde_json::Result<Self> {
        Ok(Self {
            id: attempt.id,
            alert_event_id: attempt.alert_event_id,
            contact_point_id: attempt.contact_point_id,
            delivery_key: attempt.delivery_key,
            attempt_no: attempt.attempt_no,
            status: serde_json::to_value(attempt.status)?
                .as_str()
                .unwrap_or("unknown")
                .to_string(),
            next_retry_at: attempt.next_retry_at,
            response_code: attempt.response_code,
            response_excerpt: attempt.response_excerpt,
            delivered_at: attempt.delivered_at,
            created_at: attempt.created_at,
            updated_at: attempt.updated_at,
        })
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct AlertRuleSimulationRequestPayload {
    #[schema(value_type = Object)]
    pub(crate) criteria: serde_json::Value,
    pub(crate) start: chrono::DateTime<chrono::Utc>,
    pub(crate) end: chrono::DateTime<chrono::Utc>,
    pub(crate) sample_limit: usize,
}

impl AlertRuleSimulationRequestPayload {
    pub(crate) fn into_domain(self) -> serde_json::Result<AlertSimulationRequest> {
        Ok(AlertSimulationRequest {
            criteria: serde_json::from_value::<AlertMatchCriteria>(self.criteria)?,
            start: self.start,
            end: self.end,
            sample_limit: self.sample_limit,
        })
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct AlertRuleSimulationWindowPayload {
    pub(crate) start: chrono::DateTime<chrono::Utc>,
    pub(crate) end: chrono::DateTime<chrono::Utc>,
    pub(crate) sample_limit: usize,
}

impl AlertRuleSimulationWindowPayload {
    pub(crate) fn with_criteria(self, criteria: AlertMatchCriteria) -> AlertSimulationRequest {
        AlertSimulationRequest {
            criteria,
            start: self.start,
            end: self.end,
            sample_limit: self.sample_limit,
        }
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct AlertSimulationSamplePayload {
    pub(crate) source_event_id: i64,
    pub(crate) source_kind: String,
    pub(crate) source_id: String,
    pub(crate) source_timestamp: chrono::DateTime<chrono::Utc>,
    #[schema(value_type = Object)]
    pub(crate) payload: serde_json::Value,
}

impl AlertSimulationSamplePayload {
    pub(crate) fn from_sample(sample: AlertSimulationSample) -> serde_json::Result<Self> {
        Ok(Self {
            source_event_id: sample.source_event_id,
            source_kind: serde_json::to_value(sample.source_kind)?
                .as_str()
                .unwrap_or("unknown")
                .to_string(),
            source_id: sample.source_id,
            source_timestamp: sample.source_timestamp,
            payload: sample.payload,
        })
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct AlertSimulationResultPayload {
    pub(crate) total_matches: usize,
    pub(crate) samples: Vec<AlertSimulationSamplePayload>,
}

impl AlertSimulationResultPayload {
    pub(crate) fn from_result(result: AlertSimulationResult) -> serde_json::Result<Self> {
        Ok(Self {
            total_matches: result.total_matches,
            samples: result
                .samples
                .into_iter()
                .map(AlertSimulationSamplePayload::from_sample)
                .collect::<serde_json::Result<Vec<_>>>()?,
        })
    }
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct AlertTestResponse {
    pub(crate) delivered: bool,
    pub(crate) response_code: Option<i32>,
    pub(crate) response_excerpt: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct AlertContactPointsResponse {
    pub(crate) items: Vec<AlertContactPointPayload>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct AlertRulesResponse {
    pub(crate) items: Vec<AlertRulePayload>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct AlertSilencesResponse {
    pub(crate) items: Vec<AlertSilencePayload>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct AlertRuleEventsResponse {
    pub(crate) items: Vec<AlertEventPayload>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub(crate) struct AlertDeliveriesResponse {
    pub(crate) items: Vec<AlertDeliveryAttemptPayload>,
}
