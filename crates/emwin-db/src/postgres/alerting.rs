use super::{PersistError, PersistResult, PostgresMetadataSink};
use emwin_service::{
    AlertContactPoint, AlertContactPointConfig, AlertContactPointInput, AlertDeliveryAttempt,
    AlertDeliveryStatus, AlertEvent, AlertMatchCriteria, AlertRule, AlertRuleInput,
    AlertRuleTarget, AlertSilence, AlertSilenceInput, AlertSimulationRequest,
    AlertSimulationResult, AlertSimulationSample, AlertSourceEvent, AlertSourceKind,
    IncidentChange, SourceKind,
};
use sqlx::Row;

struct DeliveryAttemptUpdate<'a> {
    id: i64,
    claimed_at: chrono::DateTime<chrono::Utc>,
    attempt_no: i32,
    status: AlertDeliveryStatus,
    next_retry_at: Option<chrono::DateTime<chrono::Utc>>,
    response_code: Option<i32>,
    response_excerpt: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct AlertContactPointRecord {
    pub id: i64,
    pub name: String,
    pub enabled: bool,
    pub config: AlertContactPointConfig,
}

impl PostgresMetadataSink {
    pub async fn list_alert_contact_points(&self) -> PersistResult<Vec<AlertContactPoint>> {
        let pool = self.ensure_pool().await?;
        let rows = sqlx::query(
            "SELECT id, name, enabled, kind, config_json, created_at, updated_at
             FROM alerting.contact_points
             ORDER BY id",
        )
        .fetch_all(&pool)
        .await?;
        rows.into_iter().map(map_alert_contact_point).collect()
    }

    pub async fn get_alert_contact_point(
        &self,
        id: i64,
    ) -> PersistResult<Option<AlertContactPoint>> {
        let pool = self.ensure_pool().await?;
        let row = sqlx::query(
            "SELECT id, name, enabled, kind, config_json, created_at, updated_at
             FROM alerting.contact_points
             WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&pool)
        .await?;
        row.map(map_alert_contact_point).transpose()
    }

    pub async fn get_alert_contact_point_record(
        &self,
        id: i64,
    ) -> PersistResult<Option<AlertContactPointRecord>> {
        let pool = self.ensure_pool().await?;
        let row = sqlx::query(
            "SELECT id, name, enabled, config_json
             FROM alerting.contact_points
             WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&pool)
        .await?;
        row.map(|row| {
            Ok(AlertContactPointRecord {
                id: row.get("id"),
                name: row.get("name"),
                enabled: row.get("enabled"),
                config: decode_json(&row, "config_json")?,
            })
        })
        .transpose()
    }

    pub async fn create_alert_contact_point(
        &self,
        input: AlertContactPointInput,
    ) -> PersistResult<AlertContactPoint> {
        let pool = self.ensure_pool().await?;
        let row = sqlx::query(
            "INSERT INTO alerting.contact_points (name, kind, enabled, config_json)
             VALUES ($1, $2, $3, $4)
             RETURNING id, name, enabled, kind, config_json, created_at, updated_at",
        )
        .bind(&input.name)
        .bind(serde_label(input.config.kind())?)
        .bind(input.enabled)
        .bind(serde_json::to_value(&input.config).map_err(invalid_request)?)
        .fetch_one(&pool)
        .await?;
        map_alert_contact_point(row)
    }

    pub async fn update_alert_contact_point(
        &self,
        id: i64,
        input: AlertContactPointInput,
    ) -> PersistResult<Option<AlertContactPoint>> {
        let pool = self.ensure_pool().await?;
        let row = sqlx::query(
            "UPDATE alerting.contact_points
             SET name = $2,
                 kind = $3,
                 enabled = $4,
                 config_json = $5,
                 updated_at = now()
             WHERE id = $1
             RETURNING id, name, enabled, kind, config_json, created_at, updated_at",
        )
        .bind(id)
        .bind(&input.name)
        .bind(serde_label(input.config.kind())?)
        .bind(input.enabled)
        .bind(serde_json::to_value(&input.config).map_err(invalid_request)?)
        .fetch_optional(&pool)
        .await?;
        row.map(map_alert_contact_point).transpose()
    }

    pub async fn delete_alert_contact_point(&self, id: i64) -> PersistResult<bool> {
        let pool = self.ensure_pool().await?;
        let done = sqlx::query("DELETE FROM alerting.contact_points WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }

    pub async fn list_alert_rules(&self) -> PersistResult<Vec<AlertRule>> {
        let pool = self.ensure_pool().await?;
        let rows = sqlx::query(
            "SELECT id, name, enabled, criteria_json, trigger_policy_json, template_json, created_at, updated_at
             FROM alerting.rules
             ORDER BY id",
        )
        .fetch_all(&pool)
        .await?;
        let mut rules = Vec::with_capacity(rows.len());
        for row in rows {
            rules.push(self.map_alert_rule_row(row).await?);
        }
        Ok(rules)
    }

    pub async fn list_enabled_alert_rules(
        &self,
        source_kind: AlertSourceKind,
    ) -> PersistResult<Vec<AlertRule>> {
        let pool = self.ensure_pool().await?;
        let rows = sqlx::query(
            "SELECT id, name, enabled, criteria_json, trigger_policy_json, template_json, created_at, updated_at
             FROM alerting.rules
             WHERE enabled = TRUE AND source_kind = $1
             ORDER BY id",
        )
        .bind(serde_label(source_kind)?)
        .fetch_all(&pool)
        .await?;
        let mut rules = Vec::with_capacity(rows.len());
        for row in rows {
            rules.push(self.map_alert_rule_row(row).await?);
        }
        Ok(rules)
    }

    pub async fn get_alert_rule(&self, id: i64) -> PersistResult<Option<AlertRule>> {
        let pool = self.ensure_pool().await?;
        let row = sqlx::query(
            "SELECT id, name, enabled, criteria_json, trigger_policy_json, template_json, created_at, updated_at
             FROM alerting.rules
             WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&pool)
        .await?;
        match row {
            Some(row) => Ok(Some(self.map_alert_rule_row(row).await?)),
            None => Ok(None),
        }
    }

    pub async fn create_alert_rule(&self, input: AlertRuleInput) -> PersistResult<AlertRule> {
        let pool = self.ensure_pool().await?;
        let source_kind = match &input.criteria {
            AlertMatchCriteria::ProductAvailable(_) => AlertSourceKind::ProductAvailable,
            AlertMatchCriteria::IncidentChange(_) => AlertSourceKind::IncidentChange,
        };
        let mut tx = pool.begin().await?;
        let row = sqlx::query(
            "INSERT INTO alerting.rules (
                name, source_kind, enabled, criteria_json, trigger_policy_json, template_json
             ) VALUES ($1, $2, $3, $4, $5, $6)
             RETURNING id, name, enabled, criteria_json, trigger_policy_json, template_json, created_at, updated_at",
        )
        .bind(&input.name)
        .bind(serde_label(source_kind)?)
        .bind(input.enabled)
        .bind(serde_json::to_value(&input.criteria).map_err(invalid_request)?)
        .bind(serde_json::to_value(&input.trigger_policy).map_err(invalid_request)?)
        .bind(serde_json::to_value(&input.template).map_err(invalid_request)?)
        .fetch_one(&mut *tx)
        .await?;
        let rule_id: i64 = row.get("id");
        replace_rule_targets(&mut tx, rule_id, &input.targets).await?;
        tx.commit().await?;
        self.get_alert_rule(rule_id).await?.ok_or_else(|| {
            PersistError::InvalidRequest("created alert rule could not be reloaded".into())
        })
    }

    pub async fn update_alert_rule(
        &self,
        id: i64,
        input: AlertRuleInput,
    ) -> PersistResult<Option<AlertRule>> {
        let pool = self.ensure_pool().await?;
        let source_kind = match &input.criteria {
            AlertMatchCriteria::ProductAvailable(_) => AlertSourceKind::ProductAvailable,
            AlertMatchCriteria::IncidentChange(_) => AlertSourceKind::IncidentChange,
        };
        let mut tx = pool.begin().await?;
        let row = sqlx::query(
            "UPDATE alerting.rules
             SET name = $2,
                 source_kind = $3,
                 enabled = $4,
                 criteria_json = $5,
                 trigger_policy_json = $6,
                 template_json = $7,
                 updated_at = now()
             WHERE id = $1
             RETURNING id",
        )
        .bind(id)
        .bind(&input.name)
        .bind(serde_label(source_kind)?)
        .bind(input.enabled)
        .bind(serde_json::to_value(&input.criteria).map_err(invalid_request)?)
        .bind(serde_json::to_value(&input.trigger_policy).map_err(invalid_request)?)
        .bind(serde_json::to_value(&input.template).map_err(invalid_request)?)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(_) = row else {
            tx.rollback().await?;
            return Ok(None);
        };
        replace_rule_targets(&mut tx, id, &input.targets).await?;
        tx.commit().await?;
        self.get_alert_rule(id).await
    }

    pub async fn delete_alert_rule(&self, id: i64) -> PersistResult<bool> {
        let pool = self.ensure_pool().await?;
        let done = sqlx::query("DELETE FROM alerting.rules WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }

    pub async fn list_alert_silences(&self) -> PersistResult<Vec<AlertSilence>> {
        let pool = self.ensure_pool().await?;
        let rows = sqlx::query(
            "SELECT id, criteria_json, starts_at, ends_at, reason, created_at
             FROM alerting.silences
             ORDER BY starts_at DESC, id DESC",
        )
        .fetch_all(&pool)
        .await?;
        rows.into_iter().map(map_alert_silence).collect()
    }

    pub async fn list_active_alert_silences(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> PersistResult<Vec<AlertSilence>> {
        let pool = self.ensure_pool().await?;
        let rows = sqlx::query(
            "SELECT id, criteria_json, starts_at, ends_at, reason, created_at
             FROM alerting.silences
             WHERE starts_at <= $1 AND ends_at >= $1
             ORDER BY id",
        )
        .bind(now)
        .fetch_all(&pool)
        .await?;
        rows.into_iter().map(map_alert_silence).collect()
    }

    pub async fn create_alert_silence(
        &self,
        input: AlertSilenceInput,
    ) -> PersistResult<AlertSilence> {
        let pool = self.ensure_pool().await?;
        let row = sqlx::query(
            "INSERT INTO alerting.silences (criteria_json, starts_at, ends_at, reason)
             VALUES ($1, $2, $3, $4)
             RETURNING id, criteria_json, starts_at, ends_at, reason, created_at",
        )
        .bind(serde_json::to_value(&input.criteria).map_err(invalid_request)?)
        .bind(input.starts_at)
        .bind(input.ends_at)
        .bind(&input.reason)
        .fetch_one(&pool)
        .await?;
        map_alert_silence(row)
    }

    pub async fn delete_alert_silence(&self, id: i64) -> PersistResult<bool> {
        let pool = self.ensure_pool().await?;
        let done = sqlx::query("DELETE FROM alerting.silences WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }

    pub async fn list_alert_rule_events(&self, rule_id: i64) -> PersistResult<Vec<AlertEvent>> {
        let pool = self.ensure_pool().await?;
        let rows = sqlx::query(
            "SELECT id, rule_id, source_event_id, delivery_key, severity, title, body, payload_json, created_at
             FROM alerting.events
             WHERE rule_id = $1
             ORDER BY id DESC",
        )
        .bind(rule_id)
        .fetch_all(&pool)
        .await?;
        rows.into_iter().map(map_alert_event).collect()
    }

    pub async fn get_alert_event(&self, id: i64) -> PersistResult<Option<AlertEvent>> {
        let pool = self.ensure_pool().await?;
        let row = sqlx::query(
            "SELECT id, rule_id, source_event_id, delivery_key, severity, title, body, payload_json, created_at
             FROM alerting.events
             WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&pool)
        .await?;
        row.map(map_alert_event).transpose()
    }

    pub async fn list_alert_deliveries(&self) -> PersistResult<Vec<AlertDeliveryAttempt>> {
        let pool = self.ensure_pool().await?;
        let rows = sqlx::query(
            "SELECT id, alert_event_id, contact_point_id, delivery_key, attempt_no, status, claimed_at, next_retry_at, response_code, response_excerpt, delivered_at, created_at, updated_at
             FROM alerting.delivery_attempts
             ORDER BY id DESC",
        )
        .fetch_all(&pool)
        .await?;
        rows.into_iter().map(map_alert_delivery_attempt).collect()
    }

    pub async fn claim_pending_alert_source_events(
        &self,
        limit: i64,
        claim_lease: chrono::Duration,
    ) -> PersistResult<Vec<AlertSourceEvent>> {
        let pool = self.ensure_pool().await?;
        let lease_cutoff = chrono::Utc::now() - claim_lease;
        let rows = sqlx::query(
            "WITH claimed AS (
                SELECT id
                FROM alerting.source_events
                WHERE processed_at IS NULL
                  AND (claimed_at IS NULL OR claimed_at <= $2)
                ORDER BY id
                LIMIT $1
                FOR UPDATE SKIP LOCKED
             )
             UPDATE alerting.source_events source_events
             SET claimed_at = now()
             FROM claimed
             WHERE source_events.id = claimed.id
             RETURNING source_events.id, source_events.source_kind, source_events.source_id, source_events.payload_json, source_events.source_timestamp, source_events.created_at, source_events.claimed_at, source_events.processed_at",
        )
        .bind(limit)
        .bind(lease_cutoff)
        .fetch_all(&pool)
        .await?;
        rows.into_iter().map(map_alert_source_event).collect()
    }

    pub async fn mark_alert_source_event_processed(
        &self,
        id: i64,
        claimed_at: chrono::DateTime<chrono::Utc>,
    ) -> PersistResult<bool> {
        let pool = self.ensure_pool().await?;
        let done = sqlx::query(
            "UPDATE alerting.source_events
             SET processed_at = now()
             WHERE id = $1 AND claimed_at = $2",
        )
        .bind(id)
        .bind(claimed_at)
        .execute(&pool)
        .await?;
        Ok(done.rows_affected() > 0)
    }

    pub async fn insert_alert_event_with_attempts(
        &self,
        rule: &AlertRule,
        source_event_id: i64,
        delivery_key: &str,
        title: &str,
        body: &str,
        payload: serde_json::Value,
    ) -> PersistResult<Option<AlertEvent>> {
        let pool = self.ensure_pool().await?;
        let mut tx = pool.begin().await?;
        let inserted = sqlx::query(
            "INSERT INTO alerting.events (
                rule_id, source_event_id, delivery_key, severity, title, body, payload_json
             ) VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (delivery_key) DO NOTHING
             RETURNING id, rule_id, source_event_id, delivery_key, severity, title, body, payload_json, created_at",
        )
        .bind(rule.id)
        .bind(source_event_id)
        .bind(delivery_key)
        .bind(rule.trigger_policy.severity.as_deref())
        .bind(title)
        .bind(body)
        .bind(payload)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = inserted else {
            tx.rollback().await?;
            return Ok(None);
        };
        let event = map_alert_event(row)?;
        for target in &rule.targets {
            sqlx::query(
                "INSERT INTO alerting.delivery_attempts (
                    alert_event_id, contact_point_id, delivery_key, attempt_no, status
                 ) VALUES ($1, $2, $3, 0, 'pending')",
            )
            .bind(event.id)
            .bind(target.contact_point_id)
            .bind(format!("{delivery_key}:{}", target.contact_point_id))
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(Some(event))
    }

    pub async fn rule_has_recent_alert_event(
        &self,
        rule_id: i64,
        source_id: &str,
        since: chrono::DateTime<chrono::Utc>,
    ) -> PersistResult<bool> {
        let pool = self.ensure_pool().await?;
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                SELECT 1
                FROM alerting.events events
                JOIN alerting.source_events source_events
                  ON source_events.id = events.source_event_id
                WHERE events.rule_id = $1
                  AND source_events.source_id = $2
                  AND events.created_at >= $3
            )",
        )
        .bind(rule_id)
        .bind(source_id)
        .bind(since)
        .fetch_one(&pool)
        .await?;
        Ok(exists)
    }

    pub async fn first_alert_source_event_timestamp(
        &self,
        source_kind: AlertSourceKind,
    ) -> PersistResult<Option<chrono::DateTime<chrono::Utc>>> {
        let pool = self.ensure_pool().await?;
        sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>(
            "SELECT source_timestamp
             FROM alerting.source_events
             WHERE source_kind = $1
             ORDER BY source_timestamp ASC, id ASC
             LIMIT 1",
        )
        .bind(serde_label(source_kind)?)
        .fetch_optional(&pool)
        .await
        .map_err(Into::into)
    }

    pub async fn claim_due_delivery_attempts(
        &self,
        limit: i64,
        claim_lease: chrono::Duration,
    ) -> PersistResult<Vec<AlertDeliveryAttempt>> {
        let pool = self.ensure_pool().await?;
        let lease_cutoff = chrono::Utc::now() - claim_lease;
        let rows = sqlx::query(
            "WITH claimed AS (
                SELECT id
                FROM alerting.delivery_attempts
                WHERE (
                    status IN ('pending', 'retry_pending')
                    AND (next_retry_at IS NULL OR next_retry_at <= now())
                  )
                  OR (
                    status = 'in_progress'
                    AND claimed_at <= $2
                  )
                ORDER BY id
                LIMIT $1
                FOR UPDATE SKIP LOCKED
             )
             UPDATE alerting.delivery_attempts delivery_attempts
             SET status = 'in_progress',
                 claimed_at = now(),
                 updated_at = now()
             FROM claimed
             WHERE delivery_attempts.id = claimed.id
             RETURNING delivery_attempts.id, delivery_attempts.alert_event_id, delivery_attempts.contact_point_id, delivery_attempts.delivery_key, delivery_attempts.attempt_no, delivery_attempts.status, delivery_attempts.claimed_at, delivery_attempts.next_retry_at, delivery_attempts.response_code, delivery_attempts.response_excerpt, delivery_attempts.delivered_at, delivery_attempts.created_at, delivery_attempts.updated_at",
        )
        .bind(limit)
        .bind(lease_cutoff)
        .fetch_all(&pool)
        .await?;
        rows.into_iter().map(map_alert_delivery_attempt).collect()
    }

    pub async fn mark_delivery_attempt_delivered(
        &self,
        id: i64,
        claimed_at: chrono::DateTime<chrono::Utc>,
        attempt_no: i32,
        response_code: Option<i32>,
        response_excerpt: Option<&str>,
    ) -> PersistResult<bool> {
        self.update_delivery_attempt(DeliveryAttemptUpdate {
            id,
            claimed_at,
            attempt_no,
            status: AlertDeliveryStatus::Delivered,
            next_retry_at: None,
            response_code,
            response_excerpt,
        })
        .await
    }

    pub async fn mark_delivery_attempt_retry(
        &self,
        id: i64,
        claimed_at: chrono::DateTime<chrono::Utc>,
        attempt_no: i32,
        next_retry_at: chrono::DateTime<chrono::Utc>,
        response_code: Option<i32>,
        response_excerpt: Option<&str>,
    ) -> PersistResult<bool> {
        self.update_delivery_attempt(DeliveryAttemptUpdate {
            id,
            claimed_at,
            attempt_no,
            status: AlertDeliveryStatus::RetryPending,
            next_retry_at: Some(next_retry_at),
            response_code,
            response_excerpt,
        })
        .await
    }

    pub async fn mark_delivery_attempt_failed(
        &self,
        id: i64,
        claimed_at: chrono::DateTime<chrono::Utc>,
        attempt_no: i32,
        response_code: Option<i32>,
        response_excerpt: Option<&str>,
    ) -> PersistResult<bool> {
        self.update_delivery_attempt(DeliveryAttemptUpdate {
            id,
            claimed_at,
            attempt_no,
            status: AlertDeliveryStatus::Failed,
            next_retry_at: None,
            response_code,
            response_excerpt,
        })
        .await
    }

    pub async fn simulate_alerts(
        &self,
        request: &AlertSimulationRequest,
    ) -> PersistResult<AlertSimulationResult> {
        let pool = self.ensure_pool().await?;
        let rows = sqlx::query(
            "SELECT id, source_kind, source_id, payload_json, source_timestamp, created_at, claimed_at, processed_at
             FROM alerting.source_events
             WHERE source_timestamp >= $1 AND source_timestamp <= $2
             ORDER BY source_timestamp DESC, id DESC",
        )
        .bind(request.start)
        .bind(request.end)
        .fetch_all(&pool)
        .await?;

        let mut total_matches = 0usize;
        let mut samples = Vec::new();
        for row in rows {
            let event = map_alert_source_event(row)?;
            if !self
                .event_matches_criteria(&request.criteria, &event)
                .await?
            {
                continue;
            }
            total_matches += 1;
            if samples.len() < request.sample_limit {
                samples.push(AlertSimulationSample {
                    source_event_id: event.id,
                    source_kind: event.source_kind,
                    source_id: event.source_id,
                    source_timestamp: event.source_timestamp,
                    payload: event.payload,
                });
            }
        }

        Ok(AlertSimulationResult {
            total_matches,
            samples,
        })
    }

    pub async fn event_matches_criteria(
        &self,
        criteria: &AlertMatchCriteria,
        event: &AlertSourceEvent,
    ) -> PersistResult<bool> {
        match criteria {
            AlertMatchCriteria::ProductAvailable(input) => {
                if event.source_kind != AlertSourceKind::ProductAvailable {
                    return Ok(false);
                }
                let metadata = self.load_product_metadata_for_source_event(event).await?;
                let filter = emwin_service::FileEventFilter::try_from_input(input.as_ref())
                    .map_err(|err| PersistError::InvalidRequest(err.message))?;
                Ok(filter.matches_metadata(&metadata))
            }
            AlertMatchCriteria::IncidentChange(input) => {
                if event.source_kind != AlertSourceKind::IncidentChange {
                    return Ok(false);
                }
                let change: IncidentChange =
                    serde_json::from_value(event.payload.clone()).map_err(invalid_request)?;
                Ok(incident_filter_matches(input.as_ref(), &change))
            }
        }
    }

    pub async fn load_product_metadata_for_source_event(
        &self,
        event: &AlertSourceEvent,
    ) -> PersistResult<emwin_service::CompletedFileMetadata> {
        let product_id = event
            .source_id
            .parse::<i64>()
            .map_err(|err| PersistError::InvalidRequest(err.to_string()))?;
        let pool = self.ensure_pool().await?;
        let row = sqlx::query(
            "SELECT filename, source_timestamp_utc, source_receiver, source_message_id, payload_location
             FROM products
             WHERE id = $1",
        )
        .bind(product_id)
        .fetch_one(&pool)
        .await?;
        let filename: String = row.get("filename");
        let timestamp_utc: i64 = row.get("source_timestamp_utc");
        let payload_location: String = row.get("payload_location");
        let source_receiver: String = row.get("source_receiver");
        let source_message_id: Option<String> = row.get("source_message_id");
        let bytes = self.blob_reader.read(&payload_location).await?;
        Ok(crate::build_completed_file_metadata(
            &filename,
            u64::try_from(timestamp_utc).map_err(|_| {
                PersistError::InvalidRequest(format!(
                    "negative source timestamp for product {product_id}"
                ))
            })?,
            source_kind_from_product_row(&source_receiver, source_message_id),
            &bytes,
        ))
    }

    async fn map_alert_rule_row(&self, row: sqlx::postgres::PgRow) -> PersistResult<AlertRule> {
        let id: i64 = row.get("id");
        let pool = self.ensure_pool().await?;
        let target_rows = sqlx::query(
            "SELECT contact_point_id, position
             FROM alerting.rule_targets
             WHERE rule_id = $1
             ORDER BY position, contact_point_id",
        )
        .bind(id)
        .fetch_all(&pool)
        .await?;
        Ok(AlertRule {
            id,
            name: row.get("name"),
            enabled: row.get("enabled"),
            criteria: decode_json(&row, "criteria_json")?,
            trigger_policy: decode_json(&row, "trigger_policy_json")?,
            template: decode_json(&row, "template_json")?,
            targets: target_rows
                .into_iter()
                .map(|target| AlertRuleTarget {
                    contact_point_id: target.get("contact_point_id"),
                    position: target.get("position"),
                })
                .collect(),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }

    async fn update_delivery_attempt(
        &self,
        update: DeliveryAttemptUpdate<'_>,
    ) -> PersistResult<bool> {
        let pool = self.ensure_pool().await?;
        let done = sqlx::query(
            "UPDATE alerting.delivery_attempts
             SET attempt_no = $2,
                 status = $3,
                 next_retry_at = $4,
                 response_code = $5,
                 response_excerpt = $6,
                 claimed_at = CASE WHEN $3 = 'delivered' THEN claimed_at ELSE NULL END,
                 delivered_at = CASE WHEN $3 = 'delivered' THEN now() ELSE delivered_at END,
                 updated_at = now()
             WHERE id = $1 AND claimed_at = $7 AND status = 'in_progress'",
        )
        .bind(update.id)
        .bind(update.attempt_no)
        .bind(serde_label(update.status)?)
        .bind(update.next_retry_at)
        .bind(update.response_code)
        .bind(update.response_excerpt)
        .bind(update.claimed_at)
        .execute(&pool)
        .await?;
        Ok(done.rows_affected() > 0)
    }
}

async fn replace_rule_targets(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    rule_id: i64,
    targets: &[AlertRuleTarget],
) -> PersistResult<()> {
    sqlx::query("DELETE FROM alerting.rule_targets WHERE rule_id = $1")
        .bind(rule_id)
        .execute(&mut **tx)
        .await?;
    for target in targets {
        sqlx::query(
            "INSERT INTO alerting.rule_targets (rule_id, contact_point_id, position)
             VALUES ($1, $2, $3)",
        )
        .bind(rule_id)
        .bind(target.contact_point_id)
        .bind(target.position)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

fn map_alert_contact_point(row: sqlx::postgres::PgRow) -> PersistResult<AlertContactPoint> {
    let config: AlertContactPointConfig = decode_json(&row, "config_json")?;
    Ok(AlertContactPoint {
        id: row.get("id"),
        name: row.get("name"),
        enabled: row.get("enabled"),
        kind: config.kind(),
        config: config.redact(),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

fn map_alert_silence(row: sqlx::postgres::PgRow) -> PersistResult<AlertSilence> {
    Ok(AlertSilence {
        id: row.get("id"),
        criteria: decode_json(&row, "criteria_json")?,
        starts_at: row.get("starts_at"),
        ends_at: row.get("ends_at"),
        reason: row.get("reason"),
        created_at: row.get("created_at"),
    })
}

fn map_alert_event(row: sqlx::postgres::PgRow) -> PersistResult<AlertEvent> {
    Ok(AlertEvent {
        id: row.get("id"),
        rule_id: row.get("rule_id"),
        source_event_id: row.get("source_event_id"),
        delivery_key: row.get("delivery_key"),
        severity: row.get("severity"),
        title: row.get("title"),
        body: row.get("body"),
        payload: row.get("payload_json"),
        created_at: row.get("created_at"),
    })
}

fn map_alert_delivery_attempt(row: sqlx::postgres::PgRow) -> PersistResult<AlertDeliveryAttempt> {
    let status_str: String = row.get("status");
    Ok(AlertDeliveryAttempt {
        id: row.get("id"),
        alert_event_id: row.get("alert_event_id"),
        contact_point_id: row.get("contact_point_id"),
        delivery_key: row.get("delivery_key"),
        attempt_no: row.get("attempt_no"),
        status: serde_json::from_value(serde_json::Value::String(status_str))
            .map_err(invalid_request)?,
        claimed_at: row.get("claimed_at"),
        next_retry_at: row.get("next_retry_at"),
        response_code: row.get("response_code"),
        response_excerpt: row.get("response_excerpt"),
        delivered_at: row.get("delivered_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

fn map_alert_source_event(row: sqlx::postgres::PgRow) -> PersistResult<AlertSourceEvent> {
    let source_kind: String = row.get("source_kind");
    Ok(AlertSourceEvent {
        id: row.get("id"),
        source_kind: serde_json::from_value(serde_json::Value::String(source_kind))
            .map_err(invalid_request)?,
        source_id: row.get("source_id"),
        payload: row.get("payload_json"),
        source_timestamp: row.get("source_timestamp"),
        created_at: row.get("created_at"),
        claimed_at: row.get("claimed_at"),
        processed_at: row.get("processed_at"),
    })
}

fn decode_json<T>(row: &sqlx::postgres::PgRow, field: &str) -> PersistResult<T>
where
    T: for<'de> serde::Deserialize<'de>,
{
    serde_json::from_value(row.get(field)).map_err(invalid_request)
}

fn invalid_request(err: impl std::fmt::Display) -> PersistError {
    PersistError::InvalidRequest(err.to_string())
}

fn serde_label<T: serde::Serialize>(value: T) -> PersistResult<String> {
    serde_json::to_value(value)
        .map_err(invalid_request)?
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| {
            PersistError::InvalidRequest("serde value did not serialize to string".into())
        })
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

fn source_kind_from_product_row(
    source_receiver: &str,
    source_message_id: Option<String>,
) -> SourceKind {
    match source_receiver {
        "qbt" => SourceKind::Qbt,
        "wxwire" => SourceKind::WxWire {
            message_id: source_message_id.unwrap_or_default(),
            subject: String::new(),
            delay_stamp_utc: None,
        },
        _ => SourceKind::Unknown,
    }
}
