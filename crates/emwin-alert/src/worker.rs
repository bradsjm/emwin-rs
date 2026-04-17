use crate::error::{AlertError, AlertResult};
use chrono::{Duration as ChronoDuration, Utc};
use emwin_db::{AlertContactPointRecord, PostgresMetadataSink};
use emwin_service::{
    AlertContactPointConfig, AlertDeliveryAttempt, AlertEvent, AlertMatchCriteria, AlertRule,
    AlertSourceEvent, AlertSourceKind, CompletedFileMetadata, IncidentChange,
};
use hmac::{Hmac, Mac};
use minijinja::{Environment, context};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::RetryTransientMiddleware;
use reqwest_retry::policies::ExponentialBackoff;
use sha2::Sha256;
use std::time::Duration;
use tokio::sync::watch;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone)]
pub struct AlertWorkerConfig {
    pub source_batch_size: i64,
    pub delivery_batch_size: i64,
    pub idle_poll_interval: Duration,
    pub source_claim_lease: Duration,
    pub delivery_claim_lease: Duration,
    pub http_timeout: Duration,
    pub max_delivery_attempts: i32,
    pub apprise_api_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AlertDispatchConfig {
    pub apprise_api_url: Option<String>,
    pub http_timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct AlertDispatchOutcome {
    pub response_code: Option<i32>,
    pub response_excerpt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TestAlertNotification {
    pub title: String,
    pub body: String,
}

#[derive(Debug)]
enum DeliveryFailure {
    Retryable {
        response_code: Option<i32>,
        response_excerpt: Option<String>,
    },
    Terminal {
        response_code: Option<i32>,
        response_excerpt: Option<String>,
    },
}

impl Default for AlertWorkerConfig {
    fn default() -> Self {
        Self {
            source_batch_size: 32,
            delivery_batch_size: 32,
            idle_poll_interval: Duration::from_secs(2),
            source_claim_lease: Duration::from_secs(300),
            delivery_claim_lease: Duration::from_secs(300),
            http_timeout: Duration::from_secs(30),
            max_delivery_attempts: 4,
            apprise_api_url: None,
        }
    }
}

pub async fn run_worker(
    sink: PostgresMetadataSink,
    config: AlertWorkerConfig,
    mut shutdown_rx: watch::Receiver<bool>,
) -> AlertResult<()> {
    if config.source_batch_size <= 0 || config.delivery_batch_size <= 0 {
        return Err(AlertError::InvalidConfig(
            "batch sizes must be positive".into(),
        ));
    }
    if config.max_delivery_attempts <= 0 {
        return Err(AlertError::InvalidConfig(
            "max delivery attempts must be positive".into(),
        ));
    }
    if config.source_claim_lease.is_zero()
        || config.delivery_claim_lease.is_zero()
        || config.http_timeout.is_zero()
    {
        return Err(AlertError::InvalidConfig(
            "claim leases and HTTP timeout must be positive".into(),
        ));
    }

    let client = build_client(config.http_timeout)?;

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => break,
            result = run_once(&sink, &config, &client) => {
                if let Err(err) = result {
                    tracing::warn!(error = %err, "alert worker iteration failed");
                }
                tokio::time::sleep(config.idle_poll_interval).await;
            }
        }
    }

    Ok(())
}

pub async fn send_test_notification(
    contact_point: &AlertContactPointConfig,
    config: &AlertDispatchConfig,
    notification: &TestAlertNotification,
) -> AlertResult<AlertDispatchOutcome> {
    let client = build_client(config.http_timeout)?;
    deliver_test_notification(&client, config, contact_point, notification).await
}

async fn run_once(
    sink: &PostgresMetadataSink,
    config: &AlertWorkerConfig,
    client: &ClientWithMiddleware,
) -> AlertResult<()> {
    process_source_events(sink, config).await?;
    process_delivery_attempts(sink, config, client).await?;
    Ok(())
}

async fn process_source_events(
    sink: &PostgresMetadataSink,
    config: &AlertWorkerConfig,
) -> AlertResult<()> {
    let events = sink
        .claim_pending_alert_source_events(
            config.source_batch_size,
            chrono_duration(config.source_claim_lease)?,
        )
        .await?;
    if events.is_empty() {
        return Ok(());
    }

    let mut qbt_rules = None;
    let mut incident_rules = None;
    let env = Environment::new();
    for event in events {
        let metadata = if event.source_kind == AlertSourceKind::ProductAvailable {
            Some(sink.load_product_metadata_for_source_event(&event).await?)
        } else {
            None
        };
        let rules = match event.source_kind {
            AlertSourceKind::ProductAvailable => {
                if qbt_rules.is_none() {
                    qbt_rules = Some(sink.list_enabled_alert_rules(event.source_kind).await?);
                }
                qbt_rules.as_ref().expect("rules cache should be populated")
            }
            AlertSourceKind::IncidentChange => {
                if incident_rules.is_none() {
                    incident_rules = Some(sink.list_enabled_alert_rules(event.source_kind).await?);
                }
                incident_rules
                    .as_ref()
                    .expect("rules cache should be populated")
            }
        };
        let silences = sink.list_active_alert_silences(Utc::now()).await?;
        for rule in rules {
            if !event_matches_criteria(&rule.criteria, &event, metadata.as_ref())? {
                continue;
            }
            if is_silenced(&event, metadata.as_ref(), &silences)? {
                continue;
            }
            if rule.trigger_policy.cooldown_secs > 0 {
                let since = Utc::now()
                    - ChronoDuration::seconds(
                        i64::try_from(rule.trigger_policy.cooldown_secs).unwrap_or(i64::MAX),
                    );
                if sink
                    .rule_has_recent_alert_event(rule.id, &event.source_id, since)
                    .await?
                {
                    continue;
                }
            }
            let (title, body) = render_rule(&env, rule, &event)?;
            let delivery_key = build_delivery_key(rule, &event);
            let _ = sink
                .insert_alert_event_with_attempts(
                    rule,
                    event.id,
                    &delivery_key,
                    &title,
                    &body,
                    event.payload.clone(),
                )
                .await?;
        }
        let Some(claimed_at) = event.claimed_at else {
            return Err(AlertError::InvalidConfig(format!(
                "claimed source event {} did not include claimed_at",
                event.id
            )));
        };
        if !sink
            .mark_alert_source_event_processed(event.id, claimed_at)
            .await?
        {
            tracing::warn!(
                source_event_id = event.id,
                "skipped source event finalization because claim lease was lost"
            );
        }
    }

    Ok(())
}

async fn process_delivery_attempts(
    sink: &PostgresMetadataSink,
    config: &AlertWorkerConfig,
    client: &ClientWithMiddleware,
) -> AlertResult<()> {
    let attempts = sink
        .claim_due_delivery_attempts(
            config.delivery_batch_size,
            chrono_duration(config.delivery_claim_lease)?,
        )
        .await?;
    let dispatch_config = AlertDispatchConfig {
        apprise_api_url: config.apprise_api_url.clone(),
        http_timeout: config.http_timeout,
    };

    for attempt in attempts {
        let Some(claimed_at) = attempt.claimed_at else {
            return Err(AlertError::InvalidConfig(format!(
                "claimed delivery attempt {} did not include claimed_at",
                attempt.id
            )));
        };
        let Some(event) = sink.get_alert_event(attempt.alert_event_id).await? else {
            let _ = sink
                .mark_delivery_attempt_failed(
                    attempt.id,
                    claimed_at,
                    attempt.attempt_no + 1,
                    None,
                    Some("missing alert event"),
                )
                .await?;
            continue;
        };
        let Some(contact_point) = sink
            .get_alert_contact_point_record(attempt.contact_point_id)
            .await?
        else {
            let _ = sink
                .mark_delivery_attempt_failed(
                    attempt.id,
                    claimed_at,
                    attempt.attempt_no + 1,
                    None,
                    Some("missing contact point"),
                )
                .await?;
            continue;
        };
        if !contact_point.enabled {
            let _ = sink
                .mark_delivery_attempt_failed(
                    attempt.id,
                    claimed_at,
                    attempt.attempt_no + 1,
                    None,
                    Some("contact point disabled"),
                )
                .await?;
            continue;
        }

        match deliver_attempt(client, &dispatch_config, &event, &contact_point, &attempt).await {
            Ok(outcome) => {
                let finalized = sink
                    .mark_delivery_attempt_delivered(
                        attempt.id,
                        claimed_at,
                        attempt.attempt_no + 1,
                        outcome.response_code,
                        outcome.response_excerpt.as_deref(),
                    )
                    .await?;
                if !finalized {
                    tracing::warn!(
                        delivery_attempt_id = attempt.id,
                        "skipped delivery success finalization because claim lease was lost"
                    );
                }
            }
            Err(DeliveryFailure::Retryable {
                response_code,
                response_excerpt,
            }) => {
                let next_attempt_no = attempt.attempt_no + 1;
                if next_attempt_no >= config.max_delivery_attempts {
                    let _ = sink
                        .mark_delivery_attempt_failed(
                            attempt.id,
                            claimed_at,
                            next_attempt_no,
                            response_code,
                            response_excerpt.as_deref(),
                        )
                        .await?;
                } else {
                    let _ = sink
                        .mark_delivery_attempt_retry(
                            attempt.id,
                            claimed_at,
                            next_attempt_no,
                            Utc::now() + ChronoDuration::seconds(backoff_seconds(next_attempt_no)),
                            response_code,
                            response_excerpt.as_deref(),
                        )
                        .await?;
                }
            }
            Err(DeliveryFailure::Terminal {
                response_code,
                response_excerpt,
            }) => {
                let _ = sink
                    .mark_delivery_attempt_failed(
                        attempt.id,
                        claimed_at,
                        attempt.attempt_no + 1,
                        response_code,
                        response_excerpt.as_deref(),
                    )
                    .await?;
            }
        }
    }

    Ok(())
}

fn is_silenced(
    event: &AlertSourceEvent,
    metadata: Option<&CompletedFileMetadata>,
    silences: &[emwin_service::AlertSilence],
) -> AlertResult<bool> {
    for silence in silences {
        if event_matches_criteria(&silence.criteria, event, metadata)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn event_matches_criteria(
    criteria: &AlertMatchCriteria,
    event: &AlertSourceEvent,
    product_metadata: Option<&CompletedFileMetadata>,
) -> AlertResult<bool> {
    match criteria {
        AlertMatchCriteria::ProductAvailable(input) => {
            if event.source_kind != AlertSourceKind::ProductAvailable {
                return Ok(false);
            }
            let Some(metadata) = product_metadata else {
                return Ok(false);
            };
            let filter = emwin_service::FileEventFilter::try_from_input(input.as_ref())
                .map_err(|err| AlertError::InvalidConfig(err.message))?;
            Ok(filter.matches_metadata(metadata))
        }
        AlertMatchCriteria::IncidentChange(input) => {
            if event.source_kind != AlertSourceKind::IncidentChange {
                return Ok(false);
            }
            let change: IncidentChange = serde_json::from_value(event.payload.clone())?;
            Ok(incident_filter_matches(input.as_ref(), &change))
        }
    }
}

fn render_rule(
    env: &Environment<'_>,
    rule: &AlertRule,
    event: &AlertSourceEvent,
) -> AlertResult<(String, String)> {
    let title = env.render_str(
        &rule.template.title,
        context! {
            source_event => event.payload.clone(),
            source_kind => serde_json::to_value(event.source_kind)?,
            rule_id => rule.id,
            rule_name => rule.name.clone(),
        },
    )?;
    let body = env.render_str(
        &rule.template.body,
        context! {
            source_event => event.payload.clone(),
            source_kind => serde_json::to_value(event.source_kind)?,
            rule_id => rule.id,
            rule_name => rule.name.clone(),
        },
    )?;
    Ok((title, body))
}

fn incident_filter_matches(
    input: &emwin_service::IncidentFilterInput,
    change: &IncidentChange,
) -> bool {
    matches_csv(
        input.action.as_deref(),
        incident_action_name(change.action),
        normalize_lower,
    ) && matches_csv(
        input.office.as_deref(),
        &change.incident.office,
        normalize_upper,
    ) && matches_csv(
        input.phenomena.as_deref(),
        &change.incident.phenomena,
        normalize_upper,
    ) && matches_csv(
        input.significance.as_deref(),
        &change.incident.significance,
        normalize_upper,
    ) && matches_csv(
        input.status.as_deref(),
        &change.incident.current_status,
        normalize_lower,
    ) && matches_i64_csv(input.etn.as_deref(), change.incident.etn)
}

fn incident_action_name(action: emwin_service::IncidentChangeAction) -> &'static str {
    match action {
        emwin_service::IncidentChangeAction::Created => "created",
        emwin_service::IncidentChangeAction::Updated => "updated",
    }
}

fn matches_csv(raw: Option<&str>, actual: &str, normalize: fn(&str) -> String) -> bool {
    match raw {
        Some(raw) => raw
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(normalize)
            .any(|candidate| candidate == normalize(actual)),
        None => true,
    }
}

fn matches_i64_csv(raw: Option<&str>, actual: i64) -> bool {
    match raw {
        Some(raw) => raw
            .split(',')
            .map(str::trim)
            .filter_map(|value| value.parse::<i64>().ok())
            .any(|candidate| candidate == actual),
        None => true,
    }
}

fn normalize_lower(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn normalize_upper(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

fn build_delivery_key(rule: &AlertRule, event: &AlertSourceEvent) -> String {
    match event.source_kind {
        AlertSourceKind::ProductAvailable => {
            format!("{}:product_available:{}:fire", rule.id, event.source_id)
        }
        AlertSourceKind::IncidentChange => {
            format!(
                "{}:incident_change:{}:{}:fire",
                rule.id, event.source_id, event.id
            )
        }
    }
}

async fn deliver_attempt(
    client: &ClientWithMiddleware,
    config: &AlertDispatchConfig,
    event: &AlertEvent,
    contact_point: &AlertContactPointRecord,
    attempt: &AlertDeliveryAttempt,
) -> Result<AlertDispatchOutcome, DeliveryFailure> {
    match &contact_point.config {
        AlertContactPointConfig::Webhook {
            url,
            authorization_header,
            signing_secret,
            timeout_secs,
        } => {
            let payload = serde_json::json!({
                "delivery_key": attempt.delivery_key,
                "alert_event_id": event.id,
                "contact_point_id": contact_point.id,
                "rule_id": event.rule_id,
                "source_event_id": event.source_event_id,
                "severity": event.severity,
                "title": event.title,
                "body": event.body,
                "payload": event.payload,
            });
            // The signature covers the exact serialized JSON body sent over the wire.
            let mut request = client
                .post(url)
                .header("X-Emwin-Alert-Id", event.id.to_string())
                .header("X-Emwin-Delivery-Key", &attempt.delivery_key)
                .header("X-Emwin-Contact-Point-Id", contact_point.id.to_string())
                .json(&payload);
            if let Some(header) = authorization_header {
                request = request.header("Authorization", header);
            }
            if let Some(secret) = signing_secret {
                let signature =
                    sign_payload(secret, &payload).map_err(|err| DeliveryFailure::Terminal {
                        response_code: None,
                        response_excerpt: Some(err.to_string()),
                    })?;
                request = request.header("X-Emwin-Signature", signature);
            }
            if let Some(timeout_secs) = timeout_secs {
                request = request.timeout(Duration::from_secs(*timeout_secs));
            }
            let response = request.send().await.map_err(classify_transport_error)?;
            let status = response.status();
            let body = response.text().await.ok();
            if status.is_success() {
                Ok(AlertDispatchOutcome {
                    response_code: Some(i32::from(status.as_u16())),
                    response_excerpt: truncate(body),
                })
            } else {
                Err(classify_response(status, body))
            }
        }
        AlertContactPointConfig::Apprise { destination_url } => {
            let apprise_api_url =
                config
                    .apprise_api_url
                    .as_ref()
                    .ok_or_else(|| DeliveryFailure::Terminal {
                        response_code: None,
                        response_excerpt: Some(
                            AlertError::InvalidConfig(
                                "apprise_api_url is required for apprise contact points".into(),
                            )
                            .to_string(),
                        ),
                    })?;
            let response = client
                .post(format!("{}/notify", apprise_api_url.trim_end_matches('/')))
                .json(&serde_json::json!({
                    "urls": destination_url,
                    "title": event.title,
                    "body": event.body,
                    "type": "info",
                }))
                .send()
                .await
                .map_err(classify_transport_error)?;
            let status = response.status();
            let body = response.text().await.ok();
            if status.is_success() {
                Ok(AlertDispatchOutcome {
                    response_code: Some(i32::from(status.as_u16())),
                    response_excerpt: truncate(body),
                })
            } else {
                Err(classify_response(status, body))
            }
        }
    }
}

async fn deliver_test_notification(
    client: &ClientWithMiddleware,
    config: &AlertDispatchConfig,
    contact_point: &AlertContactPointConfig,
    notification: &TestAlertNotification,
) -> AlertResult<AlertDispatchOutcome> {
    match contact_point {
        AlertContactPointConfig::Webhook {
            url,
            authorization_header,
            signing_secret,
            timeout_secs,
        } => {
            let payload = serde_json::json!({
                "event": "test",
                "title": notification.title,
                "body": notification.body,
            });
            let mut request = client.post(url).json(&payload);
            if let Some(header) = authorization_header {
                request = request.header("Authorization", header);
            }
            if let Some(secret) = signing_secret {
                request = request.header("X-Emwin-Signature", sign_payload(secret, &payload)?);
            }
            if let Some(timeout_secs) = timeout_secs {
                request = request.timeout(Duration::from_secs(*timeout_secs));
            }
            let response = request.send().await?;
            let status = response.status();
            let body = truncate(response.text().await.ok());
            if status.is_success() {
                Ok(AlertDispatchOutcome {
                    response_code: Some(i32::from(status.as_u16())),
                    response_excerpt: body,
                })
            } else {
                Err(AlertError::InvalidConfig(format!(
                    "test webhook delivery failed status={status}"
                )))
            }
        }
        AlertContactPointConfig::Apprise { destination_url } => {
            let apprise_api_url = config.apprise_api_url.as_ref().ok_or_else(|| {
                AlertError::InvalidConfig(
                    "apprise_api_url is required for apprise contact point tests".into(),
                )
            })?;
            let response = client
                .post(format!("{}/notify", apprise_api_url.trim_end_matches('/')))
                .json(&serde_json::json!({
                    "urls": destination_url,
                    "title": notification.title,
                    "body": notification.body,
                    "type": "info",
                }))
                .send()
                .await?;
            let status = response.status();
            let body = truncate(response.text().await.ok());
            if status.is_success() {
                Ok(AlertDispatchOutcome {
                    response_code: Some(i32::from(status.as_u16())),
                    response_excerpt: body,
                })
            } else {
                Err(AlertError::InvalidConfig(format!(
                    "test apprise delivery failed status={status}"
                )))
            }
        }
    }
}

fn build_client(timeout: Duration) -> AlertResult<ClientWithMiddleware> {
    let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);
    Ok(
        ClientBuilder::new(reqwest::Client::builder().timeout(timeout).build()?)
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build(),
    )
}

fn chrono_duration(value: Duration) -> AlertResult<chrono::Duration> {
    chrono::Duration::from_std(value)
        .map_err(|err| AlertError::InvalidConfig(format!("invalid duration: {err}")))
}

fn classify_transport_error(err: reqwest_middleware::Error) -> DeliveryFailure {
    DeliveryFailure::Retryable {
        response_code: None,
        response_excerpt: Some(err.to_string()),
    }
}

fn classify_response(status: reqwest::StatusCode, body: Option<String>) -> DeliveryFailure {
    let response_code = Some(i32::from(status.as_u16()));
    let response_excerpt = truncate(body);
    if status.is_server_error() || matches!(status.as_u16(), 408 | 425 | 429) {
        DeliveryFailure::Retryable {
            response_code,
            response_excerpt,
        }
    } else {
        DeliveryFailure::Terminal {
            response_code,
            response_excerpt,
        }
    }
}

fn sign_payload(secret: &str, payload: &serde_json::Value) -> AlertResult<String> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|err| AlertError::InvalidConfig(err.to_string()))?;
    mac.update(serde_json::to_string(payload)?.as_bytes());
    Ok(mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn backoff_seconds(attempt_no: i32) -> i64 {
    match attempt_no {
        0 | 1 => 5,
        2 => 15,
        3 => 45,
        _ => 90,
    }
}

fn truncate(value: Option<String>) -> Option<String> {
    value.map(|value| value.chars().take(256).collect())
}

#[cfg(test)]
mod tests {
    use super::{
        AlertContactPointConfig, AlertDispatchConfig, TestAlertNotification, backoff_seconds,
        build_delivery_key, send_test_notification, sign_payload,
    };
    use chrono::Utc;
    use emwin_service::{
        AlertMatchCriteria, AlertRule, AlertRuleTarget, AlertSourceEvent, AlertSourceKind,
        AlertTemplate, AlertTriggerPolicy, FileFilterInput,
    };
    use std::time::Duration;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn webhook_test_send_succeeds() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/hook"))
            .and(header("authorization", "Bearer secret"))
            .and(body_string_contains("emwin-rs test"))
            .respond_with(ResponseTemplate::new(204).set_body_string("ok"))
            .mount(&server)
            .await;

        let outcome = send_test_notification(
            &AlertContactPointConfig::Webhook {
                url: format!("{}/hook", server.uri()),
                authorization_header: Some("Bearer secret".to_string()),
                signing_secret: Some("signing-key".to_string()),
                timeout_secs: Some(5),
            },
            &AlertDispatchConfig {
                apprise_api_url: None,
                http_timeout: Duration::from_secs(30),
            },
            &TestAlertNotification {
                title: "emwin-rs test".to_string(),
                body: "test body".to_string(),
            },
        )
        .await
        .expect("test webhook send should succeed");

        assert_eq!(outcome.response_code, Some(204));
    }

    #[test]
    fn payload_signature_is_hex_encoded() {
        let signature = sign_payload("secret", &serde_json::json!({"k":"v"}))
            .expect("signature should be computed");
        assert_eq!(signature.len(), 64);
        assert!(signature.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn backoff_schedule_is_stable() {
        assert_eq!(backoff_seconds(1), 5);
        assert_eq!(backoff_seconds(2), 15);
        assert_eq!(backoff_seconds(3), 45);
        assert_eq!(backoff_seconds(4), 90);
    }

    #[test]
    fn delivery_key_keeps_product_identity_stable() {
        let rule = test_rule(7);
        let event = test_source_event(AlertSourceKind::ProductAvailable, 10, "42");

        assert_eq!(
            build_delivery_key(&rule, &event),
            "7:product_available:42:fire"
        );
    }

    #[test]
    fn delivery_key_includes_incident_source_event_id() {
        let rule = test_rule(7);
        let first = test_source_event(AlertSourceKind::IncidentChange, 10, "KOAX/FF/W/1:updated");
        let second = test_source_event(AlertSourceKind::IncidentChange, 11, "KOAX/FF/W/1:updated");

        assert_ne!(
            build_delivery_key(&rule, &first),
            build_delivery_key(&rule, &second)
        );
        assert_eq!(
            build_delivery_key(&rule, &first),
            "7:incident_change:KOAX/FF/W/1:updated:10:fire"
        );
    }

    #[tokio::test]
    async fn default_http_timeout_bounds_webhook_test_send() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/slow"))
            .respond_with(ResponseTemplate::new(204).set_delay(Duration::from_millis(200)))
            .mount(&server)
            .await;

        let started = tokio::time::Instant::now();
        let result = send_test_notification(
            &AlertContactPointConfig::Webhook {
                url: format!("{}/slow", server.uri()),
                authorization_header: None,
                signing_secret: None,
                timeout_secs: None,
            },
            &AlertDispatchConfig {
                apprise_api_url: None,
                http_timeout: Duration::from_millis(20),
            },
            &TestAlertNotification {
                title: "timeout".to_string(),
                body: "timeout".to_string(),
            },
        )
        .await;

        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    fn test_rule(id: i64) -> AlertRule {
        let now = Utc::now();
        AlertRule {
            id,
            name: "test rule".to_string(),
            enabled: true,
            criteria: AlertMatchCriteria::ProductAvailable(Box::<FileFilterInput>::default()),
            trigger_policy: AlertTriggerPolicy {
                cooldown_secs: 0,
                severity: None,
            },
            template: AlertTemplate {
                title: "title".to_string(),
                body: "body".to_string(),
            },
            targets: vec![AlertRuleTarget {
                contact_point_id: 1,
                position: 0,
            }],
            created_at: now,
            updated_at: now,
        }
    }

    fn test_source_event(
        source_kind: AlertSourceKind,
        id: i64,
        source_id: &str,
    ) -> AlertSourceEvent {
        let now = Utc::now();
        AlertSourceEvent {
            id,
            source_kind,
            source_id: source_id.to_string(),
            payload: serde_json::json!({}),
            source_timestamp: now,
            created_at: now,
            claimed_at: Some(now),
            processed_at: None,
        }
    }
}
