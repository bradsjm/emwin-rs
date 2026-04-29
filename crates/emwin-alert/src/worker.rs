#![allow(missing_docs)]

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
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::Instant;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone)]
pub struct AlertWorkerConfig {
    pub source_batch_size: i64,
    pub delivery_batch_size: i64,
    pub idle_poll_interval: Duration,
    pub stats_log_interval: Duration,
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

#[derive(Debug, Default)]
pub struct AlertWorkerStats {
    pub source_events_claimed_total: AtomicU64,
    pub source_events_processed_total: AtomicU64,
    pub alert_events_created_total: AtomicU64,
    pub alert_matches_silenced_total: AtomicU64,
    pub alert_matches_cooldown_suppressed_total: AtomicU64,
    pub source_claim_lost_total: AtomicU64,
    pub delivery_attempts_claimed_total: AtomicU64,
    pub delivery_success_total: AtomicU64,
    pub delivery_retry_scheduled_total: AtomicU64,
    pub delivery_terminal_failure_total: AtomicU64,
    pub delivery_finalization_claim_lost_total: AtomicU64,
    pub delivery_missing_alert_event_total: AtomicU64,
    pub delivery_missing_contact_point_total: AtomicU64,
    pub delivery_disabled_contact_point_total: AtomicU64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AlertWorkerStatsSnapshot {
    pub source_events_claimed_total: u64,
    pub source_events_processed_total: u64,
    pub alert_events_created_total: u64,
    pub alert_matches_silenced_total: u64,
    pub alert_matches_cooldown_suppressed_total: u64,
    pub source_claim_lost_total: u64,
    pub delivery_attempts_claimed_total: u64,
    pub delivery_success_total: u64,
    pub delivery_retry_scheduled_total: u64,
    pub delivery_terminal_failure_total: u64,
    pub delivery_finalization_claim_lost_total: u64,
    pub delivery_missing_alert_event_total: u64,
    pub delivery_missing_contact_point_total: u64,
    pub delivery_disabled_contact_point_total: u64,
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
            stats_log_interval: Duration::from_secs(30),
            source_claim_lease: Duration::from_secs(300),
            delivery_claim_lease: Duration::from_secs(300),
            http_timeout: Duration::from_secs(30),
            max_delivery_attempts: 4,
            apprise_api_url: None,
        }
    }
}

impl AlertWorkerStats {
    pub fn snapshot(&self) -> AlertWorkerStatsSnapshot {
        AlertWorkerStatsSnapshot {
            source_events_claimed_total: self.source_events_claimed_total.load(Ordering::Relaxed),
            source_events_processed_total: self
                .source_events_processed_total
                .load(Ordering::Relaxed),
            alert_events_created_total: self.alert_events_created_total.load(Ordering::Relaxed),
            alert_matches_silenced_total: self.alert_matches_silenced_total.load(Ordering::Relaxed),
            alert_matches_cooldown_suppressed_total: self
                .alert_matches_cooldown_suppressed_total
                .load(Ordering::Relaxed),
            source_claim_lost_total: self.source_claim_lost_total.load(Ordering::Relaxed),
            delivery_attempts_claimed_total: self
                .delivery_attempts_claimed_total
                .load(Ordering::Relaxed),
            delivery_success_total: self.delivery_success_total.load(Ordering::Relaxed),
            delivery_retry_scheduled_total: self
                .delivery_retry_scheduled_total
                .load(Ordering::Relaxed),
            delivery_terminal_failure_total: self
                .delivery_terminal_failure_total
                .load(Ordering::Relaxed),
            delivery_finalization_claim_lost_total: self
                .delivery_finalization_claim_lost_total
                .load(Ordering::Relaxed),
            delivery_missing_alert_event_total: self
                .delivery_missing_alert_event_total
                .load(Ordering::Relaxed),
            delivery_missing_contact_point_total: self
                .delivery_missing_contact_point_total
                .load(Ordering::Relaxed),
            delivery_disabled_contact_point_total: self
                .delivery_disabled_contact_point_total
                .load(Ordering::Relaxed),
        }
    }
}

impl AlertWorkerStatsSnapshot {
    fn log(self, message: &'static str) {
        tracing::info!(
            source_events_claimed_total = self.source_events_claimed_total,
            source_events_processed_total = self.source_events_processed_total,
            alert_events_created_total = self.alert_events_created_total,
            alert_matches_silenced_total = self.alert_matches_silenced_total,
            alert_matches_cooldown_suppressed_total = self.alert_matches_cooldown_suppressed_total,
            source_claim_lost_total = self.source_claim_lost_total,
            delivery_attempts_claimed_total = self.delivery_attempts_claimed_total,
            delivery_success_total = self.delivery_success_total,
            delivery_retry_scheduled_total = self.delivery_retry_scheduled_total,
            delivery_terminal_failure_total = self.delivery_terminal_failure_total,
            delivery_finalization_claim_lost_total = self.delivery_finalization_claim_lost_total,
            delivery_missing_alert_event_total = self.delivery_missing_alert_event_total,
            delivery_missing_contact_point_total = self.delivery_missing_contact_point_total,
            delivery_disabled_contact_point_total = self.delivery_disabled_contact_point_total,
            "{message}"
        );
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
    let stats = AlertWorkerStats::default();
    let mut last_stats_log_at = Instant::now();

    log_worker_started(&config);

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => break,
            result = run_once(&sink, &config, &client, &stats) => {
                if let Err(err) = result {
                    tracing::warn!(error = %err, "alert worker iteration failed");
                }
                maybe_log_stats(&stats, config.stats_log_interval, &mut last_stats_log_at);
                tokio::select! {
                    _ = shutdown_rx.changed() => break,
                    _ = tokio::time::sleep(config.idle_poll_interval) => {}
                }
            }
        }
    }

    stats.snapshot().log("alert worker stopped");
    Ok(())
}

fn maybe_log_stats(stats: &AlertWorkerStats, interval: Duration, last_logged_at: &mut Instant) {
    if interval.is_zero() || last_logged_at.elapsed() < interval {
        return;
    }

    stats.snapshot().log("alert worker stats snapshot");
    *last_logged_at = Instant::now();
}

fn log_worker_started(config: &AlertWorkerConfig) {
    tracing::info!(
        source_batch_size = config.source_batch_size,
        delivery_batch_size = config.delivery_batch_size,
        idle_poll_interval_secs = config.idle_poll_interval.as_secs(),
        stats_log_interval_secs = config.stats_log_interval.as_secs(),
        source_claim_lease_secs = config.source_claim_lease.as_secs(),
        delivery_claim_lease_secs = config.delivery_claim_lease.as_secs(),
        http_timeout_secs = config.http_timeout.as_secs(),
        max_delivery_attempts = config.max_delivery_attempts,
        apprise_enabled = config.apprise_api_url.is_some(),
        "alert worker started"
    );
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
    stats: &AlertWorkerStats,
) -> AlertResult<()> {
    process_source_events(sink, config, stats).await?;
    process_delivery_attempts(sink, config, client, stats).await?;
    Ok(())
}

async fn process_source_events(
    sink: &PostgresMetadataSink,
    config: &AlertWorkerConfig,
    stats: &AlertWorkerStats,
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
    stats
        .source_events_claimed_total
        .fetch_add(events.len() as u64, Ordering::Relaxed);
    tracing::info!(
        claimed_source_events = events.len(),
        source_claim_lease_secs = config.source_claim_lease.as_secs(),
        "claimed alert source event batch"
    );

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
            if let Some(silence_id) = matching_silence(&event, metadata.as_ref(), &silences)? {
                stats
                    .alert_matches_silenced_total
                    .fetch_add(1, Ordering::Relaxed);
                log_silenced_match(&event, rule, silence_id);
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
                    stats
                        .alert_matches_cooldown_suppressed_total
                        .fetch_add(1, Ordering::Relaxed);
                    log_cooldown_suppressed_match(&event, rule, rule.trigger_policy.cooldown_secs);
                    continue;
                }
            }
            let (title, body) = render_rule(&env, rule, &event)?;
            let delivery_key = build_delivery_key(rule, &event);
            let inserted = sink
                .insert_alert_event_with_attempts(
                    rule,
                    event.id,
                    &delivery_key,
                    &title,
                    &body,
                    event.payload.clone(),
                )
                .await?;
            if let Some(alert_event) = inserted {
                stats
                    .alert_events_created_total
                    .fetch_add(1, Ordering::Relaxed);
                tracing::info!(
                    source_event_id = event.id,
                    source_kind = ?event.source_kind,
                    rule_id = rule.id,
                    alert_event_id = alert_event.id,
                    delivery_key = %delivery_key,
                    "created alert event"
                );
            }
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
            let source_claim_lost_total = stats
                .source_claim_lost_total
                .fetch_add(1, Ordering::Relaxed)
                + 1;
            tracing::warn!(
                source_event_id = event.id,
                source_kind = ?event.source_kind,
                source_id = %event.source_id,
                source_claim_lost_total,
                "skipped source event finalization because claim lease was lost"
            );
        } else {
            stats
                .source_events_processed_total
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    Ok(())
}

async fn process_delivery_attempts(
    sink: &PostgresMetadataSink,
    config: &AlertWorkerConfig,
    client: &ClientWithMiddleware,
    stats: &AlertWorkerStats,
) -> AlertResult<()> {
    let attempts = sink
        .claim_due_delivery_attempts(
            config.delivery_batch_size,
            chrono_duration(config.delivery_claim_lease)?,
        )
        .await?;
    if attempts.is_empty() {
        return Ok(());
    }
    stats
        .delivery_attempts_claimed_total
        .fetch_add(attempts.len() as u64, Ordering::Relaxed);
    tracing::info!(
        claimed_delivery_attempts = attempts.len(),
        delivery_claim_lease_secs = config.delivery_claim_lease.as_secs(),
        "claimed alert delivery attempt batch"
    );
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
            stats
                .delivery_missing_alert_event_total
                .fetch_add(1, Ordering::Relaxed);
            log_missing_alert_event(
                attempt.id,
                attempt.alert_event_id,
                attempt.contact_point_id,
                attempt.attempt_no + 1,
            );
            let finalized = sink
                .mark_delivery_attempt_failed(
                    attempt.id,
                    claimed_at,
                    attempt.attempt_no + 1,
                    None,
                    Some("missing alert event"),
                )
                .await?;
            log_delivery_finalization_claim_loss(
                stats,
                finalized,
                attempt.id,
                attempt.alert_event_id,
                attempt.contact_point_id,
                attempt.attempt_no + 1,
                "missing alert event",
            );
            continue;
        };
        let Some(contact_point) = sink
            .get_alert_contact_point_record(attempt.contact_point_id)
            .await?
        else {
            stats
                .delivery_missing_contact_point_total
                .fetch_add(1, Ordering::Relaxed);
            log_missing_contact_point(
                attempt.id,
                event.id,
                attempt.contact_point_id,
                attempt.attempt_no + 1,
            );
            let finalized = sink
                .mark_delivery_attempt_failed(
                    attempt.id,
                    claimed_at,
                    attempt.attempt_no + 1,
                    None,
                    Some("missing contact point"),
                )
                .await?;
            log_delivery_finalization_claim_loss(
                stats,
                finalized,
                attempt.id,
                event.id,
                attempt.contact_point_id,
                attempt.attempt_no + 1,
                "missing contact point",
            );
            continue;
        };
        if !contact_point.enabled {
            stats
                .delivery_disabled_contact_point_total
                .fetch_add(1, Ordering::Relaxed);
            log_disabled_contact_point(
                attempt.id,
                event.id,
                &contact_point,
                attempt.attempt_no + 1,
            );
            let finalized = sink
                .mark_delivery_attempt_failed(
                    attempt.id,
                    claimed_at,
                    attempt.attempt_no + 1,
                    None,
                    Some("contact point disabled"),
                )
                .await?;
            log_delivery_finalization_claim_loss(
                stats,
                finalized,
                attempt.id,
                event.id,
                contact_point.id,
                attempt.attempt_no + 1,
                "contact point disabled",
            );
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
                if finalized {
                    stats.delivery_success_total.fetch_add(1, Ordering::Relaxed);
                    tracing::info!(
                        delivery_attempt_id = attempt.id,
                        alert_event_id = event.id,
                        contact_point_id = contact_point.id,
                        contact_point_kind = ?contact_point.config.kind(),
                        attempt_no = attempt.attempt_no + 1,
                        response_code = outcome.response_code,
                        "alert delivery succeeded"
                    );
                } else {
                    log_delivery_finalization_claim_loss(
                        stats,
                        finalized,
                        attempt.id,
                        event.id,
                        contact_point.id,
                        attempt.attempt_no + 1,
                        "delivery success",
                    );
                }
            }
            Err(DeliveryFailure::Retryable {
                response_code,
                response_excerpt,
            }) => {
                let next_attempt_no = attempt.attempt_no + 1;
                if next_attempt_no >= config.max_delivery_attempts {
                    stats
                        .delivery_terminal_failure_total
                        .fetch_add(1, Ordering::Relaxed);
                    log_delivery_retry_exhausted(
                        &attempt,
                        &event,
                        &contact_point,
                        config.max_delivery_attempts,
                        next_attempt_no,
                        response_code,
                        response_excerpt.as_deref(),
                    );
                    let finalized = sink
                        .mark_delivery_attempt_failed(
                            attempt.id,
                            claimed_at,
                            next_attempt_no,
                            response_code,
                            response_excerpt.as_deref(),
                        )
                        .await?;
                    log_delivery_finalization_claim_loss(
                        stats,
                        finalized,
                        attempt.id,
                        event.id,
                        contact_point.id,
                        next_attempt_no,
                        "delivery failure",
                    );
                } else {
                    let next_retry_at =
                        Utc::now() + ChronoDuration::seconds(backoff_seconds(next_attempt_no));
                    stats
                        .delivery_retry_scheduled_total
                        .fetch_add(1, Ordering::Relaxed);
                    log_delivery_retry_scheduled(
                        &attempt,
                        &event,
                        &contact_point,
                        next_attempt_no,
                        next_retry_at,
                        response_code,
                        response_excerpt.as_deref(),
                    );
                    let finalized = sink
                        .mark_delivery_attempt_retry(
                            attempt.id,
                            claimed_at,
                            next_attempt_no,
                            next_retry_at,
                            response_code,
                            response_excerpt.as_deref(),
                        )
                        .await?;
                    log_delivery_finalization_claim_loss(
                        stats,
                        finalized,
                        attempt.id,
                        event.id,
                        contact_point.id,
                        next_attempt_no,
                        "delivery retry scheduling",
                    );
                }
            }
            Err(DeliveryFailure::Terminal {
                response_code,
                response_excerpt,
            }) => {
                stats
                    .delivery_terminal_failure_total
                    .fetch_add(1, Ordering::Relaxed);
                log_delivery_terminal_failure(
                    &attempt,
                    &event,
                    &contact_point,
                    attempt.attempt_no + 1,
                    response_code,
                    response_excerpt.as_deref(),
                );
                let finalized = sink
                    .mark_delivery_attempt_failed(
                        attempt.id,
                        claimed_at,
                        attempt.attempt_no + 1,
                        response_code,
                        response_excerpt.as_deref(),
                    )
                    .await?;
                log_delivery_finalization_claim_loss(
                    stats,
                    finalized,
                    attempt.id,
                    event.id,
                    contact_point.id,
                    attempt.attempt_no + 1,
                    "delivery failure",
                );
            }
        }
    }

    Ok(())
}

fn matching_silence(
    event: &AlertSourceEvent,
    metadata: Option<&CompletedFileMetadata>,
    silences: &[emwin_service::AlertSilence],
) -> AlertResult<Option<i64>> {
    for silence in silences {
        if event_matches_criteria(&silence.criteria, event, metadata)? {
            return Ok(Some(silence.id));
        }
    }
    Ok(None)
}

fn log_silenced_match(event: &AlertSourceEvent, rule: &AlertRule, silence_id: i64) {
    tracing::info!(
        source_event_id = event.id,
        source_kind = ?event.source_kind,
        source_id = %event.source_id,
        rule_id = rule.id,
        silence_id,
        "alert match suppressed by silence"
    );
}

fn log_cooldown_suppressed_match(event: &AlertSourceEvent, rule: &AlertRule, cooldown_secs: u64) {
    tracing::info!(
        source_event_id = event.id,
        source_kind = ?event.source_kind,
        source_id = %event.source_id,
        rule_id = rule.id,
        cooldown_secs,
        "alert match suppressed by cooldown"
    );
}

fn log_delivery_finalization_claim_loss(
    stats: &AlertWorkerStats,
    finalized: bool,
    delivery_attempt_id: i64,
    alert_event_id: i64,
    contact_point_id: i64,
    attempt_no: i32,
    outcome: &'static str,
) {
    if finalized {
        return;
    }

    let delivery_finalization_claim_lost_total = stats
        .delivery_finalization_claim_lost_total
        .fetch_add(1, Ordering::Relaxed)
        + 1;
    tracing::warn!(
        delivery_attempt_id,
        alert_event_id,
        contact_point_id,
        attempt_no,
        outcome,
        delivery_finalization_claim_lost_total,
        "skipped delivery finalization because claim lease was lost"
    );
}

fn log_missing_alert_event(
    delivery_attempt_id: i64,
    alert_event_id: i64,
    contact_point_id: i64,
    attempt_no: i32,
) {
    tracing::warn!(
        delivery_attempt_id,
        alert_event_id,
        contact_point_id,
        attempt_no,
        "delivery attempt missing alert event"
    );
}

fn log_missing_contact_point(
    delivery_attempt_id: i64,
    alert_event_id: i64,
    contact_point_id: i64,
    attempt_no: i32,
) {
    tracing::warn!(
        delivery_attempt_id,
        alert_event_id,
        contact_point_id,
        attempt_no,
        "delivery attempt missing contact point"
    );
}

fn log_disabled_contact_point(
    delivery_attempt_id: i64,
    alert_event_id: i64,
    contact_point: &AlertContactPointRecord,
    attempt_no: i32,
) {
    tracing::warn!(
        delivery_attempt_id,
        alert_event_id,
        contact_point_id = contact_point.id,
        contact_point_kind = ?contact_point.config.kind(),
        attempt_no,
        "delivery attempt skipped because contact point is disabled"
    );
}

fn log_delivery_retry_scheduled(
    attempt: &AlertDeliveryAttempt,
    event: &AlertEvent,
    contact_point: &AlertContactPointRecord,
    next_attempt_no: i32,
    next_retry_at: chrono::DateTime<Utc>,
    response_code: Option<i32>,
    response_excerpt: Option<&str>,
) {
    tracing::warn!(
        delivery_attempt_id = attempt.id,
        alert_event_id = event.id,
        contact_point_id = contact_point.id,
        contact_point_kind = ?contact_point.config.kind(),
        attempt_no = attempt.attempt_no,
        next_attempt_no,
        next_retry_at = %next_retry_at,
        response_code,
        response_excerpt = response_excerpt.unwrap_or(""),
        "alert delivery retry scheduled"
    );
}

fn log_delivery_retry_exhausted(
    attempt: &AlertDeliveryAttempt,
    event: &AlertEvent,
    contact_point: &AlertContactPointRecord,
    max_delivery_attempts: i32,
    next_attempt_no: i32,
    response_code: Option<i32>,
    response_excerpt: Option<&str>,
) {
    tracing::warn!(
        delivery_attempt_id = attempt.id,
        alert_event_id = event.id,
        contact_point_id = contact_point.id,
        contact_point_kind = ?contact_point.config.kind(),
        attempt_no = attempt.attempt_no,
        next_attempt_no,
        max_delivery_attempts,
        response_code,
        response_excerpt = response_excerpt.unwrap_or(""),
        "alert delivery retryable failure reached max attempts"
    );
}

fn log_delivery_terminal_failure(
    attempt: &AlertDeliveryAttempt,
    event: &AlertEvent,
    contact_point: &AlertContactPointRecord,
    attempt_no: i32,
    response_code: Option<i32>,
    response_excerpt: Option<&str>,
) {
    tracing::warn!(
        delivery_attempt_id = attempt.id,
        alert_event_id = event.id,
        contact_point_id = contact_point.id,
        contact_point_kind = ?contact_point.config.kind(),
        attempt_no,
        response_code,
        response_excerpt = response_excerpt.unwrap_or(""),
        "alert delivery failed"
    );
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
        AlertContactPointConfig, AlertDispatchConfig, AlertWorkerConfig, AlertWorkerStats,
        TestAlertNotification, backoff_seconds, build_delivery_key, log_cooldown_suppressed_match,
        log_delivery_finalization_claim_loss, log_delivery_retry_exhausted,
        log_delivery_retry_scheduled, log_delivery_terminal_failure, log_disabled_contact_point,
        log_missing_alert_event, log_missing_contact_point, log_silenced_match, log_worker_started,
        send_test_notification, sign_payload,
    };
    use chrono::Utc;
    use emwin_db::AlertContactPointRecord;
    use emwin_service::{
        AlertDeliveryAttempt, AlertDeliveryStatus, AlertEvent, AlertMatchCriteria, AlertRule,
        AlertRuleTarget, AlertSourceEvent, AlertSourceKind, AlertTemplate, AlertTriggerPolicy,
        FileFilterInput,
    };
    use std::io::Write;
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tracing_subscriber::fmt::MakeWriter;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[derive(Clone, Debug, Default)]
    struct SharedLogBuffer(Arc<Mutex<Vec<u8>>>);

    impl SharedLogBuffer {
        fn contents(&self) -> String {
            String::from_utf8(
                self.0
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone(),
            )
            .expect("logs should be utf-8")
        }
    }

    #[derive(Clone, Debug)]
    struct LogWriter(SharedLogBuffer);

    impl Write for LogWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for SharedLogBuffer {
        type Writer = LogWriter;

        fn make_writer(&'a self) -> LogWriter {
            LogWriter(self.clone())
        }
    }

    fn capture_logs(action: impl FnOnce()) -> String {
        let buffer = SharedLogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_writer(buffer.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        action();
        buffer.contents()
    }

    #[test]
    fn stats_snapshot_reports_all_observability_counters() {
        let stats = AlertWorkerStats::default();
        stats
            .source_events_claimed_total
            .store(2, Ordering::Relaxed);
        stats
            .source_events_processed_total
            .store(1, Ordering::Relaxed);
        stats.alert_events_created_total.store(3, Ordering::Relaxed);
        stats
            .alert_matches_silenced_total
            .store(4, Ordering::Relaxed);
        stats
            .alert_matches_cooldown_suppressed_total
            .store(5, Ordering::Relaxed);
        stats.source_claim_lost_total.store(6, Ordering::Relaxed);
        stats
            .delivery_attempts_claimed_total
            .store(7, Ordering::Relaxed);
        stats.delivery_success_total.store(8, Ordering::Relaxed);
        stats
            .delivery_retry_scheduled_total
            .store(9, Ordering::Relaxed);
        stats
            .delivery_terminal_failure_total
            .store(10, Ordering::Relaxed);
        stats
            .delivery_finalization_claim_lost_total
            .store(11, Ordering::Relaxed);
        stats
            .delivery_missing_alert_event_total
            .store(12, Ordering::Relaxed);
        stats
            .delivery_missing_contact_point_total
            .store(13, Ordering::Relaxed);
        stats
            .delivery_disabled_contact_point_total
            .store(14, Ordering::Relaxed);

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.source_events_claimed_total, 2);
        assert_eq!(snapshot.source_events_processed_total, 1);
        assert_eq!(snapshot.alert_events_created_total, 3);
        assert_eq!(snapshot.alert_matches_silenced_total, 4);
        assert_eq!(snapshot.alert_matches_cooldown_suppressed_total, 5);
        assert_eq!(snapshot.source_claim_lost_total, 6);
        assert_eq!(snapshot.delivery_attempts_claimed_total, 7);
        assert_eq!(snapshot.delivery_success_total, 8);
        assert_eq!(snapshot.delivery_retry_scheduled_total, 9);
        assert_eq!(snapshot.delivery_terminal_failure_total, 10);
        assert_eq!(snapshot.delivery_finalization_claim_lost_total, 11);
        assert_eq!(snapshot.delivery_missing_alert_event_total, 12);
        assert_eq!(snapshot.delivery_missing_contact_point_total, 13);
        assert_eq!(snapshot.delivery_disabled_contact_point_total, 14);
    }

    #[test]
    fn startup_and_shutdown_logs_include_non_secret_fields() {
        let logs = capture_logs(|| {
            let config = AlertWorkerConfig {
                source_batch_size: 11,
                delivery_batch_size: 7,
                idle_poll_interval: Duration::from_secs(2),
                stats_log_interval: Duration::from_secs(30),
                source_claim_lease: Duration::from_secs(300),
                delivery_claim_lease: Duration::from_secs(120),
                http_timeout: Duration::from_secs(9),
                max_delivery_attempts: 4,
                apprise_api_url: Some("http://127.0.0.1:8000".to_string()),
            };
            log_worker_started(&config);

            let stats = AlertWorkerStats::default();
            stats.delivery_success_total.store(2, Ordering::Relaxed);
            stats.snapshot().log("alert worker stopped");
        });

        assert!(logs.contains("alert worker started"));
        assert!(logs.contains("source_batch_size=11"));
        assert!(logs.contains("delivery_batch_size=7"));
        assert!(logs.contains("apprise_enabled=true"));
        assert!(!logs.contains("http://127.0.0.1:8000"));
        assert!(logs.contains("alert worker stopped"));
        assert!(logs.contains("delivery_success_total=2"));
    }

    #[test]
    fn suppression_logs_distinguish_silence_and_cooldown() {
        let logs = capture_logs(|| {
            let rule = test_rule(7);
            let event = test_source_event(AlertSourceKind::IncidentChange, 10, "KOAX/FF/W/1");
            log_silenced_match(&event, &rule, 41);
            log_cooldown_suppressed_match(&event, &rule, 90);
        });

        assert!(logs.contains("alert match suppressed by silence"));
        assert!(logs.contains("silence_id=41"));
        assert!(logs.contains("alert match suppressed by cooldown"));
        assert!(logs.contains("cooldown_secs=90"));
    }

    #[test]
    fn delivery_finalization_claim_loss_log_includes_context() {
        let logs = capture_logs(|| {
            let stats = AlertWorkerStats::default();
            log_delivery_finalization_claim_loss(&stats, false, 9, 10, 11, 2, "delivery failure");
        });

        assert!(logs.contains("skipped delivery finalization because claim lease was lost"));
        assert!(logs.contains("delivery_attempt_id=9"));
        assert!(logs.contains("alert_event_id=10"));
        assert!(logs.contains("contact_point_id=11"));
        assert!(logs.contains("attempt_no=2"));
        assert!(logs.contains("outcome=\"delivery failure\""));
    }

    #[test]
    fn missing_or_disabled_delivery_target_logs_warning_context() {
        let logs = capture_logs(|| {
            log_missing_alert_event(1, 2, 3, 4);
            log_missing_contact_point(5, 6, 7, 8);
            log_disabled_contact_point(9, 10, &test_contact_point(false), 11);
        });

        assert!(logs.contains("delivery attempt missing alert event"));
        assert!(logs.contains("delivery attempt missing contact point"));
        assert!(logs.contains("delivery attempt skipped because contact point is disabled"));
        assert!(logs.contains("contact_point_kind=Webhook"));
    }

    #[test]
    fn delivery_retry_and_failure_logs_include_structured_fields() {
        let logs = capture_logs(|| {
            let attempt = test_delivery_attempt(17, 21, 34, 1);
            let event = test_alert_event(21, 7, 55);
            let contact_point = test_contact_point(true);
            let next_retry_at = Utc::now();

            log_delivery_retry_scheduled(
                &attempt,
                &event,
                &contact_point,
                2,
                next_retry_at,
                Some(429),
                Some("slow down"),
            );
            log_delivery_retry_exhausted(
                &attempt,
                &event,
                &contact_point,
                4,
                4,
                Some(503),
                Some("retry budget exhausted"),
            );
            log_delivery_terminal_failure(
                &attempt,
                &event,
                &contact_point,
                2,
                Some(400),
                Some("bad request"),
            );
        });

        assert!(logs.contains("alert delivery retry scheduled"));
        assert!(logs.contains("next_attempt_no=2"));
        assert!(logs.contains("response_code=429"));
        assert!(logs.contains("response_excerpt=\"slow down\""));
        assert!(logs.contains("alert delivery retryable failure reached max attempts"));
        assert!(logs.contains("max_delivery_attempts=4"));
        assert!(logs.contains("alert delivery failed"));
        assert!(logs.contains("response_code=400"));
    }

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

    fn test_alert_event(id: i64, rule_id: i64, source_event_id: i64) -> AlertEvent {
        AlertEvent {
            id,
            rule_id,
            source_event_id,
            delivery_key: format!("{rule_id}:delivery"),
            severity: Some("warning".to_string()),
            title: "title".to_string(),
            body: "body".to_string(),
            payload: serde_json::json!({"kind":"test"}),
            created_at: Utc::now(),
        }
    }

    fn test_delivery_attempt(
        id: i64,
        alert_event_id: i64,
        contact_point_id: i64,
        attempt_no: i32,
    ) -> AlertDeliveryAttempt {
        let now = Utc::now();
        AlertDeliveryAttempt {
            id,
            alert_event_id,
            contact_point_id,
            delivery_key: format!("{alert_event_id}:{contact_point_id}"),
            attempt_no,
            status: AlertDeliveryStatus::InProgress,
            claimed_at: Some(now),
            next_retry_at: None,
            response_code: None,
            response_excerpt: None,
            delivered_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn test_contact_point(enabled: bool) -> AlertContactPointRecord {
        AlertContactPointRecord {
            id: 34,
            name: "webhook".to_string(),
            enabled,
            config: AlertContactPointConfig::Webhook {
                url: "https://example.invalid/hook".to_string(),
                authorization_header: Some("Bearer secret".to_string()),
                signing_secret: Some("signing-secret".to_string()),
                timeout_secs: Some(5),
            },
        }
    }
}
