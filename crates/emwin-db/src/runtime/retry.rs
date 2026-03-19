use super::PersistenceConfig;
use crate::error::PersistError;
use std::time::{Duration, Instant};
use tracing::{info, warn};

#[derive(Debug, Default)]
pub(super) struct BackendHealth {
    degraded: Option<DegradedBackend>,
}

#[derive(Debug)]
pub(super) struct DegradedBackend {
    stage: &'static str,
    backend: &'static str,
    target: String,
    failure_class: &'static str,
    last_error: String,
    last_logged_at: Instant,
    suppressed_failures: u64,
}

#[derive(Debug, Clone)]
pub(super) struct FailureContext {
    pub(super) stage: &'static str,
    pub(super) backend: &'static str,
    pub(super) target: String,
}

impl BackendHealth {
    pub(super) fn note_retryable_failure(
        &mut self,
        request_key: &str,
        context: &FailureContext,
        err: &PersistError,
        retry_delay: Duration,
        attempt: u32,
        failure_log_cooldown: Duration,
    ) {
        let failure_class = err.failure_class();
        let error_text = err.to_string();
        let now = Instant::now();
        match self.degraded.as_mut() {
            Some(current)
                if current.failure_class == failure_class
                    && current.stage == context.stage
                    && current.backend == context.backend
                    && current.target == context.target
                    && current.last_error == error_text
                    && now.duration_since(current.last_logged_at) < failure_log_cooldown =>
            {
                current.suppressed_failures = current.suppressed_failures.saturating_add(1);
            }
            Some(current) => {
                warn!(
                    request_key = %request_key,
                    stage = context.stage,
                    backend = context.backend,
                    target = %context.target,
                    failure_class,
                    error = %err,
                    retry_delay_secs = retry_delay.as_secs(),
                    retry_attempt = attempt,
                    suppressed_failures = current.suppressed_failures,
                    "persistence backend unavailable; retrying"
                );
                *current = DegradedBackend {
                    stage: context.stage,
                    backend: context.backend,
                    target: context.target.clone(),
                    failure_class,
                    last_error: error_text,
                    last_logged_at: now,
                    suppressed_failures: 0,
                };
            }
            None => {
                warn!(
                    request_key = %request_key,
                    stage = context.stage,
                    backend = context.backend,
                    target = %context.target,
                    failure_class,
                    error = %err,
                    retry_delay_secs = retry_delay.as_secs(),
                    retry_attempt = attempt,
                    "persistence backend unavailable; retrying"
                );
                self.degraded = Some(DegradedBackend {
                    stage: context.stage,
                    backend: context.backend,
                    target: context.target.clone(),
                    failure_class,
                    last_error: error_text,
                    last_logged_at: now,
                    suppressed_failures: 0,
                });
            }
        }
    }

    pub(super) fn note_recovered(&mut self, request_key: &str) {
        let Some(degraded) = self.degraded.take() else {
            return;
        };
        info!(
            request_key = %request_key,
            stage = degraded.stage,
            backend = degraded.backend,
            target = %degraded.target,
            failure_class = degraded.failure_class,
            suppressed_failures = degraded.suppressed_failures,
            "persistence backend recovered"
        );
    }
}

pub(super) fn retry_delay(config: &PersistenceConfig, attempt: u32) -> Duration {
    let multiplier = 1u64.checked_shl(attempt.min(6)).unwrap_or(64);
    config
        .retry_initial_delay
        .saturating_mul(u32::try_from(multiplier).unwrap_or(u32::MAX))
        .min(config.retry_max_delay)
}
