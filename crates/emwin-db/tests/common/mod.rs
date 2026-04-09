#![allow(dead_code, unused_imports)]

use chrono::{DateTime, TimeZone, Utc};
use emwin_db::{
    BlobRole, BlobWriter, CompletedFileMetadata, MetadataSink, PersistedRequest, PostgresConfig,
    PostgresMetadataSink, StoredBlob, build_completed_file_metadata,
};
use emwin_service::{IncidentChange, IncidentChangeAction, IncidentChangeTrigger, SourceKind};
use sqlx::Row;
use tokio::sync::broadcast;
use tokio::time::{Duration, timeout};

#[derive(Clone, Copy)]
pub(crate) struct TestIncidentKey {
    pub(crate) office: &'static str,
    pub(crate) phenomena: &'static str,
    pub(crate) significance: &'static str,
    pub(crate) etn: i64,
}

pub(crate) struct IncidentRecord {
    pub(crate) current_status: String,
    pub(crate) latest_vtec_action: String,
    pub(crate) issued_at: DateTime<Utc>,
    pub(crate) start_utc: Option<DateTime<Utc>>,
    pub(crate) end_utc: Option<DateTime<Utc>>,
    pub(crate) first_product_id: i64,
    pub(crate) latest_product_id: i64,
    pub(crate) latest_product_timestamp_utc: DateTime<Utc>,
}

pub(crate) fn test_database_url() -> Option<String> {
    std::env::var("EMWIN_PG_TEST_DATABASE_URL").ok()
}

pub(crate) async fn connect_test_sink() -> Option<PostgresMetadataSink> {
    let database_url = test_database_url()?;
    let mut config = PostgresConfig::new(database_url);
    config.application_name = "emwin-db-test".to_string();
    Some(
        PostgresMetadataSink::connect(config)
            .await
            .expect("postgres sink should connect"),
    )
}

pub(crate) fn sample_metadata() -> CompletedFileMetadata {
    build_completed_file_metadata(
        "FFWOAXNE.TXT",
        1_704_070_800,
        SourceKind::Qbt,
        br#"000
WUUS53 KOAX 051200
FFWOAX

Flash Flood Warning
National Weather Service Omaha/Valley NE
1200 PM CST Wed Mar 5 2025

NEC001>003-051300-
/O.NEW.KOAX.FF.W.0001.250305T1200Z-250305T1800Z/
/MSRM1.3.ER.250305T1200Z.250305T1800Z.250306T0000Z.NO/

LAT...LON 4143 9613 4145 9610 4140 9608 4138 9612
TIME...MOT...LOC 1200Z 300DEG 25KT 4143 9613 4140 9608
MAXHAILSIZE...1.00 IN
MAXWINDGUST...60 MPH
"#,
    )
}

pub(crate) fn sample_blobs(_filename: &str) -> Vec<StoredBlob> {
    let payload_location =
        "file:///tmp/qbt/2023/12/31/OAX/nws_text_product/20231231T230000Z-7824e38f-FFWOAXNE.TXT";
    let sidecar_location =
        "file:///tmp/qbt/2023/12/31/OAX/nws_text_product/20231231T230000Z-7824e38f-FFWOAXNE.JSON";
    vec![
        StoredBlob {
            role: BlobRole::Payload,
            location: payload_location.to_string(),
            size_bytes: 512,
            content_type: Some("application/octet-stream".to_string()),
        },
        StoredBlob {
            role: BlobRole::MetadataSidecar,
            location: sidecar_location.to_string(),
            size_bytes: 256,
            content_type: Some("application/json".to_string()),
        },
    ]
}

pub(crate) fn sample_object_store_blobs(_filename: &str) -> Vec<StoredBlob> {
    let payload_location = "s3://example-bucket/archive/qbt/2023/12/31/OAX/nws_text_product/20231231T230000Z-7824e38f-FFWOAXNE.TXT";
    let sidecar_location = "s3://example-bucket/archive/qbt/2023/12/31/OAX/nws_text_product/20231231T230000Z-7824e38f-FFWOAXNE.JSON";
    vec![
        StoredBlob {
            role: BlobRole::Payload,
            location: payload_location.to_string(),
            size_bytes: 512,
            content_type: Some("application/octet-stream".to_string()),
        },
        StoredBlob {
            role: BlobRole::MetadataSidecar,
            location: sidecar_location.to_string(),
            size_bytes: 256,
            content_type: Some("application/json".to_string()),
        },
    ]
}

pub(crate) fn sample_filesystem_blobs_at(
    payload_location: &str,
    sidecar_location: &str,
) -> Vec<StoredBlob> {
    vec![
        StoredBlob {
            role: BlobRole::Payload,
            location: payload_location.to_string(),
            size_bytes: 512,
            content_type: Some("application/octet-stream".to_string()),
        },
        StoredBlob {
            role: BlobRole::MetadataSidecar,
            location: sidecar_location.to_string(),
            size_bytes: 256,
            content_type: Some("application/json".to_string()),
        },
    ]
}

pub(crate) fn build_vtec_metadata(
    filename: &str,
    timestamp_utc: u64,
    ugc_line: &str,
    vtec_lines: &[String],
) -> CompletedFileMetadata {
    let bulletin = format!(
        "000\nWUUS53 KOAX 051200\nFFWOAX\n\nFlash Flood Warning\nNational Weather Service Omaha/Valley NE\n1200 PM CST Wed Mar 5 2025\n\n{ugc_line}\n{}\n",
        vtec_lines.join("\n")
    );
    build_completed_file_metadata(
        filename,
        timestamp_utc,
        SourceKind::Qbt,
        bulletin.as_bytes(),
    )
}

pub(crate) fn vtec_line(status: char, action: &str, etn: i64, begin: &str, end: &str) -> String {
    format!("/{status}.{action}.KOAX.FF.W.{etn:04}.{begin}-{end}/")
}

pub(crate) fn utc_timestamp(seconds: u64) -> DateTime<Utc> {
    Utc.timestamp_opt(
        i64::try_from(seconds).expect("timestamp should fit in i64"),
        0,
    )
    .single()
    .expect("timestamp should be valid")
}

pub(crate) async fn persist_metadata(
    sink: &PostgresMetadataSink,
    metadata: CompletedFileMetadata,
) -> i64 {
    let filename = metadata.filename.clone();
    let timestamp = metadata.timestamp_utc;
    sink.persist(PersistedRequest {
        request_key: filename.clone(),
        metadata,
        blobs: sample_blobs(&filename),
    })
    .await
    .expect("postgres sink should persist metadata");

    sqlx::query("SELECT id FROM products WHERE filename = $1 AND source_timestamp_utc = $2")
        .bind(&filename)
        .bind(i64::try_from(timestamp).expect("timestamp should fit in bigint"))
        .fetch_one(&sink.pool())
        .await
        .expect("persisted product row should exist")
        .get("id")
}

pub(crate) async fn persist_metadata_with_blobs(
    sink: &PostgresMetadataSink,
    metadata: CompletedFileMetadata,
    blobs: Vec<StoredBlob>,
) -> i64 {
    let filename = metadata.filename.clone();
    let timestamp = metadata.timestamp_utc;
    sink.persist(PersistedRequest {
        request_key: filename.clone(),
        metadata,
        blobs,
    })
    .await
    .expect("postgres sink should persist metadata");

    sqlx::query("SELECT id FROM products WHERE filename = $1 AND source_timestamp_utc = $2")
        .bind(&filename)
        .bind(i64::try_from(timestamp).expect("timestamp should fit in bigint"))
        .fetch_one(&sink.pool())
        .await
        .expect("persisted product row should exist")
        .get("id")
}

pub(crate) async fn recv_incident_change(
    rx: &mut broadcast::Receiver<IncidentChange>,
) -> IncidentChange {
    timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("incident change should arrive before timeout")
        .expect("incident change should be delivered")
}

pub(crate) async fn cleanup_rows(
    sink: &PostgresMetadataSink,
    filenames: &[&str],
    incident_keys: &[TestIncidentKey],
) {
    for key in incident_keys {
        sqlx::query(
            "DELETE FROM incidents WHERE office = $1 AND phenomena = $2 AND significance = $3 AND etn = $4",
        )
        .bind(key.office)
        .bind(key.phenomena)
        .bind(key.significance)
        .bind(key.etn)
        .execute(&sink.pool())
        .await
        .expect("incident cleanup should succeed");
    }

    for filename in filenames {
        sqlx::query("DELETE FROM products WHERE filename = $1")
            .bind(*filename)
            .execute(&sink.pool())
            .await
            .expect("product cleanup should succeed");
    }
}

pub(crate) async fn fetch_incident(
    sink: &PostgresMetadataSink,
    key: TestIncidentKey,
) -> Option<IncidentRecord> {
    sqlx::query(
        "SELECT current_status, latest_vtec_action, issued_at, start_utc, end_utc, first_product_id, latest_product_id, latest_product_timestamp_utc
         FROM incidents
         WHERE office = $1 AND phenomena = $2 AND significance = $3 AND etn = $4",
    )
    .bind(key.office)
    .bind(key.phenomena)
    .bind(key.significance)
    .bind(key.etn)
    .fetch_optional(&sink.pool())
    .await
    .expect("incident query should succeed")
    .map(|row| IncidentRecord {
        current_status: row.get("current_status"),
        latest_vtec_action: row.get("latest_vtec_action"),
        issued_at: row.get("issued_at"),
        start_utc: row.get("start_utc"),
        end_utc: row.get("end_utc"),
        first_product_id: row.get("first_product_id"),
        latest_product_id: row.get("latest_product_id"),
        latest_product_timestamp_utc: row.get("latest_product_timestamp_utc"),
        })
}

pub(crate) async fn product_issue_id(sink: &PostgresMetadataSink, product_id: i64) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT id FROM product_issues WHERE product_id = $1 ORDER BY id ASC LIMIT 1",
    )
    .bind(product_id)
    .fetch_one(&sink.pool())
    .await
    .expect("product issue row should exist")
}

pub(crate) async fn update_incident_end_utc(
    sink: &PostgresMetadataSink,
    key: TestIncidentKey,
    end_utc: Option<DateTime<Utc>>,
) {
    sqlx::query(
        "UPDATE incidents SET end_utc = $5 WHERE office = $1 AND phenomena = $2 AND significance = $3 AND etn = $4",
    )
    .bind(key.office)
    .bind(key.phenomena)
    .bind(key.significance)
    .bind(key.etn)
    .bind(end_utc)
    .execute(&sink.pool())
    .await
    .expect("incident end_utc update should succeed");
}

pub(crate) async fn update_incident_status(
    sink: &PostgresMetadataSink,
    key: TestIncidentKey,
    status: &str,
) {
    sqlx::query(
        "UPDATE incidents SET current_status = $5 WHERE office = $1 AND phenomena = $2 AND significance = $3 AND etn = $4",
    )
    .bind(key.office)
    .bind(key.phenomena)
    .bind(key.significance)
    .bind(key.etn)
    .bind(status)
    .execute(&sink.pool())
    .await
    .expect("incident status update should succeed");
}
