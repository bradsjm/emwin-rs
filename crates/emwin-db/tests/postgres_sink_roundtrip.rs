mod common;

use chrono::{DateTime, Utc};
use common::*;
use emwin_service::{
    AlertDeliveryStatus, AlertMatchCriteria, AlertRuleTarget, AlertTemplate, AlertTriggerPolicy,
    FileFilterInput,
};
use sqlx::Row;
use std::collections::HashSet;

#[test]
fn postgres_migration_versions_are_unique() {
    let migrations_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut versions = HashSet::new();
    for entry in std::fs::read_dir(migrations_dir).expect("migrations directory should be readable")
    {
        let entry = entry.expect("migration entry should be readable");
        let filename = entry
            .file_name()
            .into_string()
            .expect("migration filename should be UTF-8");
        let Some((version, _description)) = filename.split_once('_') else {
            continue;
        };
        assert!(
            versions.insert(version.to_string()),
            "duplicate migration version {version}"
        );
    }
}

#[tokio::test]
async fn postgres_sink_bootstraps_and_persists_rows() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let metadata = sample_metadata();
    let incident_key = TestIncidentKey {
        office: "KOAX",
        phenomena: "FF",
        significance: "W",
        etn: 1,
    };
    cleanup_rows(&sink, &[&metadata.filename], &[incident_key]).await;

    let product_id = persist_metadata(&sink, metadata.clone()).await;

    let row = sqlx::query(
        "SELECT id, source_receiver, source_message_id, ingested_at, payload_location, metadata_location, has_vtec, has_ugc, has_hvtec, has_latlon, has_time_mot_loc, has_wind_hail
         FROM products WHERE filename = $1 AND source_timestamp_utc = $2",
    )
    .bind(&metadata.filename)
    .bind(i64::try_from(metadata.timestamp_utc).expect("timestamp should fit in bigint"))
    .fetch_one(&sink.pool())
    .await
    .expect("persisted product row should exist");

    assert_eq!(row.get::<i64, _>("id"), product_id);
    assert_eq!(row.get::<String, _>("source_receiver"), "qbt");
    assert_eq!(row.get::<Option<String>, _>("source_message_id"), None);
    assert!(row.get::<DateTime<Utc>, _>("ingested_at") <= Utc::now());
    assert_eq!(
        row.get::<String, _>("payload_location"),
        "file:///tmp/qbt/2023/12/31/OAX/nws_text_product/20231231T230000Z-7824e38f-FFWOAXNE.TXT"
    );
    assert_eq!(
        row.get::<Option<String>, _>("metadata_location").as_deref(),
        Some(
            "file:///tmp/qbt/2023/12/31/OAX/nws_text_product/20231231T230000Z-7824e38f-FFWOAXNE.JSON"
        )
    );
    assert!(row.get::<bool, _>("has_vtec"));
    assert!(row.get::<bool, _>("has_ugc"));
    assert!(row.get::<bool, _>("has_hvtec"));
    assert!(row.get::<bool, _>("has_latlon"));
    assert!(row.get::<bool, _>("has_time_mot_loc"));
    assert!(row.get::<bool, _>("has_wind_hail"));

    let origin_json_column_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM information_schema.columns WHERE table_name = 'products' AND column_name = 'origin_json'",
    )
    .fetch_one(&sink.pool())
    .await
    .expect("products schema should be queryable");
    assert_eq!(origin_json_column_count, 0);

    let summary_json_column_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM information_schema.columns WHERE table_name = 'products' AND column_name = 'summary_json'",
    )
    .fetch_one(&sink.pool())
    .await
    .expect("products schema should be queryable");
    assert_eq!(summary_json_column_count, 0);

    let pruned_summary_column_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM information_schema.columns WHERE table_name = 'products' AND column_name = ANY($1)",
    )
    .bind(vec![
        "issue_codes",
        "vtec_phenomena",
        "vtec_significance",
        "vtec_actions",
        "vtec_offices",
        "etns",
        "hvtec_nwslids",
        "hvtec_causes",
        "hvtec_severities",
        "hvtec_records",
    ])
    .fetch_one(&sink.pool())
    .await
    .expect("products schema should be queryable");
    assert_eq!(pruned_summary_column_count, 0);

    let child_product_id_index_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM pg_indexes WHERE schemaname = current_schema() AND indexname = ANY($1)",
    )
    .bind(vec![
        "product_issues_product_id_idx",
        "product_vtec_product_id_idx",
        "product_ugc_areas_product_id_idx",
        "product_hvtec_product_id_idx",
        "product_time_mot_loc_product_id_idx",
        "product_polygons_product_id_idx",
        "product_wind_hail_product_id_idx",
        "product_search_points_product_id_idx",
    ])
    .fetch_one(&sink.pool())
    .await
    .expect("child product_id indexes should be queryable");
    assert_eq!(child_product_id_index_count, 8);

    let vtec_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM product_vtec WHERE product_id = $1")
            .bind(product_id)
            .fetch_one(&sink.pool())
            .await
            .expect("vtec rows should be queryable");
    let ugc_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM product_ugc_areas WHERE product_id = $1",
    )
    .bind(product_id)
    .fetch_one(&sink.pool())
    .await
    .expect("ugc rows should be queryable");
    let hvtec_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM product_hvtec WHERE product_id = $1")
            .bind(product_id)
            .fetch_one(&sink.pool())
            .await
            .expect("hvtec rows should be queryable");
    let polygon_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM product_polygons WHERE product_id = $1")
            .bind(product_id)
            .fetch_one(&sink.pool())
            .await
            .expect("polygon rows should be queryable");
    let time_mot_loc_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM product_time_mot_loc WHERE product_id = $1",
    )
    .bind(product_id)
    .fetch_one(&sink.pool())
    .await
    .expect("time mot loc rows should be queryable");
    let wind_hail_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM product_wind_hail WHERE product_id = $1",
    )
    .bind(product_id)
    .fetch_one(&sink.pool())
    .await
    .expect("wind hail rows should be queryable");

    assert_eq!(vtec_count, 1);
    assert_eq!(ugc_count, 3);
    assert_eq!(hvtec_count, 1);
    assert_eq!(polygon_count, 1);
    assert_eq!(time_mot_loc_count, 1);
    assert_eq!(wind_hail_count, 2);

    let incident = fetch_incident(&sink, incident_key)
        .await
        .expect("incident row should exist");
    assert_eq!(incident.current_status, "active");
    assert_eq!(incident.latest_vtec_action, "NEW");
    assert_eq!(incident.first_product_id, product_id);
    assert_eq!(incident.latest_product_id, product_id);
    assert_eq!(incident.issued_at, utc_timestamp(metadata.timestamp_utc));

    cleanup_rows(&sink, &[&metadata.filename], &[incident_key]).await;
}

#[tokio::test]
async fn postgres_sink_persists_object_store_blob_locations() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let metadata = sample_metadata();
    let incident_key = TestIncidentKey {
        office: "KOAX",
        phenomena: "FF",
        significance: "W",
        etn: 1,
    };
    cleanup_rows(&sink, &[&metadata.filename], &[incident_key]).await;

    persist_metadata_with_blobs(
        &sink,
        metadata.clone(),
        sample_object_store_blobs(&metadata.filename),
    )
    .await;

    let row = sqlx::query(
        "SELECT payload_location, metadata_location
         FROM products WHERE filename = $1 AND source_timestamp_utc = $2",
    )
    .bind(&metadata.filename)
    .bind(i64::try_from(metadata.timestamp_utc).expect("timestamp should fit in bigint"))
    .fetch_one(&sink.pool())
    .await
    .expect("persisted product row should exist");

    assert_eq!(
        row.get::<String, _>("payload_location"),
        "s3://example-bucket/archive/qbt/2023/12/31/OAX/nws_text_product/20231231T230000Z-7824e38f-FFWOAXNE.TXT"
    );
    assert_eq!(
        row.get::<Option<String>, _>("metadata_location").as_deref(),
        Some(
            "s3://example-bucket/archive/qbt/2023/12/31/OAX/nws_text_product/20231231T230000Z-7824e38f-FFWOAXNE.JSON"
        )
    );

    cleanup_rows(&sink, &[&metadata.filename], &[incident_key]).await;
}

#[tokio::test]
async fn alert_source_event_claims_are_recoverable_after_lease_expiry() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };
    cleanup_alerting_rows(&sink, "source-lease").await;

    let stale_id = insert_alert_source_event(
        &sink,
        "source-lease-stale",
        Some(Utc::now() - chrono::Duration::minutes(10)),
    )
    .await;
    let fresh_id = insert_alert_source_event(&sink, "source-lease-fresh", Some(Utc::now())).await;

    let claimed = sink
        .claim_pending_alert_source_events(10, chrono::Duration::minutes(5))
        .await
        .expect("source events should be claimable");
    let claimed_ids = claimed.iter().map(|event| event.id).collect::<Vec<_>>();
    assert!(claimed_ids.contains(&stale_id));
    assert!(!claimed_ids.contains(&fresh_id));

    let claimed_stale = claimed
        .into_iter()
        .find(|event| event.id == stale_id)
        .expect("stale source event should have been claimed");
    let claimed_at = claimed_stale
        .claimed_at
        .expect("claimed source event should include claim token");
    assert!(
        sink.mark_alert_source_event_processed(stale_id, claimed_at)
            .await
            .expect("source event finalization should succeed")
    );

    let stale_token_id = insert_alert_source_event(&sink, "source-lease-token", None).await;
    let claimed_token_event = sink
        .claim_pending_alert_source_events(10, chrono::Duration::minutes(5))
        .await
        .expect("source event should be claimable")
        .into_iter()
        .find(|event| event.id == stale_token_id)
        .expect("token source event should be claimed");
    let stale_claimed_at = claimed_token_event
        .claimed_at
        .expect("claimed source event should include claim token");
    sqlx::query("UPDATE alerting.source_events SET claimed_at = now() WHERE id = $1")
        .bind(stale_token_id)
        .execute(&sink.pool())
        .await
        .expect("claim token update should succeed");
    assert!(
        !sink
            .mark_alert_source_event_processed(stale_token_id, stale_claimed_at)
            .await
            .expect("stale source event finalization should be ignored")
    );

    cleanup_alerting_rows(&sink, "source-lease").await;
}

#[tokio::test]
async fn delivery_claims_use_in_progress_leases_and_claim_tokens() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };
    cleanup_alerting_rows(&sink, "delivery-lease").await;

    let attempt_id = insert_alert_delivery_attempt(&sink, "delivery-lease").await;

    let first_claim = sink
        .claim_due_delivery_attempts(10, chrono::Duration::minutes(5))
        .await
        .expect("delivery attempt should be claimable");
    assert_eq!(first_claim.len(), 1);
    assert_eq!(first_claim[0].id, attempt_id);
    assert_eq!(first_claim[0].status, AlertDeliveryStatus::InProgress);
    let first_claimed_at = first_claim[0]
        .claimed_at
        .expect("claimed delivery should include claim token");

    let second_claim = sink
        .claim_due_delivery_attempts(10, chrono::Duration::minutes(5))
        .await
        .expect("fresh in-progress delivery should not be claimable");
    assert!(second_claim.is_empty());

    sqlx::query(
        "UPDATE alerting.delivery_attempts
         SET claimed_at = now() - interval '10 minutes'
         WHERE id = $1",
    )
    .bind(attempt_id)
    .execute(&sink.pool())
    .await
    .expect("delivery lease aging should succeed");

    let reclaimed = sink
        .claim_due_delivery_attempts(10, chrono::Duration::minutes(5))
        .await
        .expect("stale in-progress delivery should be reclaimable");
    assert_eq!(reclaimed.len(), 1);
    let second_claimed_at = reclaimed[0]
        .claimed_at
        .expect("reclaimed delivery should include claim token");
    assert_ne!(first_claimed_at, second_claimed_at);

    assert!(
        !sink
            .mark_delivery_attempt_delivered(attempt_id, first_claimed_at, 1, Some(204), Some("ok"))
            .await
            .expect("stale delivery finalization should be ignored")
    );
    assert!(
        sink.mark_delivery_attempt_retry(
            attempt_id,
            second_claimed_at,
            1,
            Utc::now() + chrono::Duration::minutes(1),
            Some(503),
            Some("retry"),
        )
        .await
        .expect("current delivery finalization should succeed")
    );

    let row =
        sqlx::query("SELECT status, claimed_at FROM alerting.delivery_attempts WHERE id = $1")
            .bind(attempt_id)
            .fetch_one(&sink.pool())
            .await
            .expect("delivery attempt should exist");
    assert_eq!(row.get::<String, _>("status"), "retry_pending");
    assert!(row.get::<Option<DateTime<Utc>>, _>("claimed_at").is_none());

    cleanup_alerting_rows(&sink, "delivery-lease").await;
}

async fn cleanup_alerting_rows(sink: &emwin_db::PostgresMetadataSink, prefix: &str) {
    sqlx::query("DELETE FROM alerting.contact_points WHERE name LIKE $1")
        .bind(format!("{prefix}%"))
        .execute(&sink.pool())
        .await
        .expect("contact point cleanup should succeed");
    sqlx::query("DELETE FROM alerting.source_events WHERE source_id LIKE $1")
        .bind(format!("{prefix}%"))
        .execute(&sink.pool())
        .await
        .expect("source event cleanup should succeed");
}

async fn insert_alert_source_event(
    sink: &emwin_db::PostgresMetadataSink,
    source_id: &str,
    claimed_at: Option<DateTime<Utc>>,
) -> i64 {
    sqlx::query(
        "INSERT INTO alerting.source_events (
            source_kind, source_id, payload_json, source_timestamp, claimed_at
         ) VALUES ('incident_change', $1, $2, now(), $3)
         RETURNING id",
    )
    .bind(source_id)
    .bind(serde_json::json!({ "test": true }))
    .bind(claimed_at)
    .fetch_one(&sink.pool())
    .await
    .expect("source event insert should succeed")
    .get("id")
}

async fn insert_alert_delivery_attempt(sink: &emwin_db::PostgresMetadataSink, prefix: &str) -> i64 {
    let source_event_id = insert_alert_source_event(sink, &format!("{prefix}-source"), None).await;
    let contact_point = sink
        .create_alert_contact_point(emwin_service::AlertContactPointInput {
            name: format!("{prefix}-contact"),
            enabled: true,
            config: emwin_service::AlertContactPointConfig::Webhook {
                url: "http://127.0.0.1/hook".to_string(),
                authorization_header: None,
                signing_secret: None,
                timeout_secs: None,
            },
        })
        .await
        .expect("contact point should be created");
    let rule = sink
        .create_alert_rule(emwin_service::AlertRuleInput {
            name: format!("{prefix}-rule"),
            enabled: true,
            criteria: AlertMatchCriteria::ProductAvailable(Box::<FileFilterInput>::default()),
            trigger_policy: AlertTriggerPolicy {
                cooldown_secs: 0,
                severity: None,
            },
            template: AlertTemplate {
                title: "test".to_string(),
                body: "test".to_string(),
            },
            targets: vec![AlertRuleTarget {
                contact_point_id: contact_point.id,
                position: 0,
            }],
        })
        .await
        .expect("rule should be created");
    let event = sink
        .insert_alert_event_with_attempts(
            &rule,
            source_event_id,
            &format!("{prefix}-delivery-key"),
            "test",
            "test",
            serde_json::json!({ "test": true }),
        )
        .await
        .expect("alert event should be inserted")
        .expect("alert event should be new");

    sqlx::query_scalar::<_, i64>(
        "SELECT id FROM alerting.delivery_attempts WHERE alert_event_id = $1",
    )
    .bind(event.id)
    .fetch_one(&sink.pool())
    .await
    .expect("delivery attempt should exist")
}
