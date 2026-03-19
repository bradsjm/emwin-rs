use chrono::{DateTime, TimeZone, Utc};
use emwin_db::{
    BlobRole, BlobStorageKind, BlobWriter, CompletedFileMetadata, IncidentChange,
    IncidentChangeAction, IncidentChangeTrigger, MetadataSink, PersistedRequest, PostgresConfig,
    PostgresMetadataSink, StoredBlob,
};
use emwin_protocol::ingest::ProductOrigin;
use sqlx::Row;
use tokio::sync::broadcast;
use tokio::time::{Duration, timeout};

#[derive(Clone, Copy)]
struct TestIncidentKey {
    office: &'static str,
    phenomena: &'static str,
    significance: &'static str,
    etn: i64,
}

struct IncidentRecord {
    current_status: String,
    latest_vtec_action: String,
    issued_at: DateTime<Utc>,
    start_utc: Option<DateTime<Utc>>,
    end_utc: Option<DateTime<Utc>>,
    first_product_id: i64,
    latest_product_id: i64,
    latest_product_timestamp_utc: DateTime<Utc>,
}

fn test_database_url() -> Option<String> {
    std::env::var("EMWIN_PG_TEST_DATABASE_URL").ok()
}

async fn connect_test_sink() -> Option<PostgresMetadataSink> {
    let database_url = test_database_url()?;
    let mut config = PostgresConfig::new(database_url);
    config.application_name = "emwin-db-test".to_string();
    Some(
        PostgresMetadataSink::connect(config)
            .await
            .expect("postgres sink should connect"),
    )
}

fn sample_metadata() -> CompletedFileMetadata {
    CompletedFileMetadata::build(
        "FFWOAXNE.TXT",
        1_704_070_800,
        ProductOrigin::Qbt,
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

fn sample_blobs(_filename: &str) -> Vec<StoredBlob> {
    let payload_location =
        "/tmp/qbt/2023/12/31/OAX/nws_text_product/20231231T230000Z-7824e38f-FFWOAXNE.TXT";
    let sidecar_location =
        "/tmp/qbt/2023/12/31/OAX/nws_text_product/20231231T230000Z-7824e38f-FFWOAXNE.JSON";
    vec![
        StoredBlob {
            kind: BlobStorageKind::Filesystem,
            role: BlobRole::Payload,
            location: payload_location.to_string(),
            size_bytes: 512,
            content_type: Some("application/octet-stream".to_string()),
        },
        StoredBlob {
            kind: BlobStorageKind::Filesystem,
            role: BlobRole::MetadataSidecar,
            location: sidecar_location.to_string(),
            size_bytes: 256,
            content_type: Some("application/json".to_string()),
        },
    ]
}

fn sample_s3_blobs(_filename: &str) -> Vec<StoredBlob> {
    let payload_location = "s3://example-bucket/archive/qbt/2023/12/31/OAX/nws_text_product/20231231T230000Z-7824e38f-FFWOAXNE.TXT";
    let sidecar_location = "s3://example-bucket/archive/qbt/2023/12/31/OAX/nws_text_product/20231231T230000Z-7824e38f-FFWOAXNE.JSON";
    vec![
        StoredBlob {
            kind: BlobStorageKind::S3,
            role: BlobRole::Payload,
            location: payload_location.to_string(),
            size_bytes: 512,
            content_type: Some("application/octet-stream".to_string()),
        },
        StoredBlob {
            kind: BlobStorageKind::S3,
            role: BlobRole::MetadataSidecar,
            location: sidecar_location.to_string(),
            size_bytes: 256,
            content_type: Some("application/json".to_string()),
        },
    ]
}

fn sample_filesystem_blobs_at(payload_location: &str, sidecar_location: &str) -> Vec<StoredBlob> {
    vec![
        StoredBlob {
            kind: BlobStorageKind::Filesystem,
            role: BlobRole::Payload,
            location: payload_location.to_string(),
            size_bytes: 512,
            content_type: Some("application/octet-stream".to_string()),
        },
        StoredBlob {
            kind: BlobStorageKind::Filesystem,
            role: BlobRole::MetadataSidecar,
            location: sidecar_location.to_string(),
            size_bytes: 256,
            content_type: Some("application/json".to_string()),
        },
    ]
}

fn build_vtec_metadata(
    filename: &str,
    timestamp_utc: u64,
    ugc_line: &str,
    vtec_lines: &[String],
) -> CompletedFileMetadata {
    let bulletin = format!(
        "000\nWUUS53 KOAX 051200\nFFWOAX\n\nFlash Flood Warning\nNational Weather Service Omaha/Valley NE\n1200 PM CST Wed Mar 5 2025\n\n{ugc_line}\n{}\n",
        vtec_lines.join("\n")
    );
    CompletedFileMetadata::build(
        filename,
        timestamp_utc,
        ProductOrigin::Qbt,
        bulletin.as_bytes(),
    )
}

fn vtec_line(status: char, action: &str, etn: i64, begin: &str, end: &str) -> String {
    format!("/{status}.{action}.KOAX.FF.W.{etn:04}.{begin}-{end}/")
}

fn utc_timestamp(seconds: u64) -> DateTime<Utc> {
    Utc.timestamp_opt(
        i64::try_from(seconds).expect("timestamp should fit in i64"),
        0,
    )
    .single()
    .expect("timestamp should be valid")
}

async fn persist_metadata(sink: &PostgresMetadataSink, metadata: CompletedFileMetadata) -> i64 {
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

async fn persist_metadata_with_blobs(
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

async fn recv_incident_change(rx: &mut broadcast::Receiver<IncidentChange>) -> IncidentChange {
    timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("incident change should arrive before timeout")
        .expect("incident change should be delivered")
}

async fn cleanup_rows(
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

async fn fetch_incident(
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

async fn update_incident_end_utc(
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

async fn update_incident_status(sink: &PostgresMetadataSink, key: TestIncidentKey, status: &str) {
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
        "/tmp/qbt/2023/12/31/OAX/nws_text_product/20231231T230000Z-7824e38f-FFWOAXNE.TXT"
    );
    assert_eq!(
        row.get::<Option<String>, _>("metadata_location").as_deref(),
        Some("/tmp/qbt/2023/12/31/OAX/nws_text_product/20231231T230000Z-7824e38f-FFWOAXNE.JSON")
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
async fn postgres_sink_persists_s3_blob_locations() {
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

    persist_metadata_with_blobs(&sink, metadata.clone(), sample_s3_blobs(&metadata.filename)).await;

    let row = sqlx::query(
        "SELECT payload_storage_kind, payload_location, metadata_storage_kind, metadata_location
         FROM products WHERE filename = $1 AND source_timestamp_utc = $2",
    )
    .bind(&metadata.filename)
    .bind(i64::try_from(metadata.timestamp_utc).expect("timestamp should fit in bigint"))
    .fetch_one(&sink.pool())
    .await
    .expect("persisted product row should exist");

    assert_eq!(row.get::<String, _>("payload_storage_kind"), "s3");
    assert_eq!(
        row.get::<String, _>("payload_location"),
        "s3://example-bucket/archive/qbt/2023/12/31/OAX/nws_text_product/20231231T230000Z-7824e38f-FFWOAXNE.TXT"
    );
    assert_eq!(
        row.get::<Option<String>, _>("metadata_storage_kind")
            .as_deref(),
        Some("s3")
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
async fn incident_projection_tracks_lifecycle_and_lineage() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let key = TestIncidentKey {
        office: "KOAX",
        phenomena: "FF",
        significance: "W",
        etn: 2001,
    };
    let filenames = [
        "FFWOAX-LIFECYCLE-NEW.TXT",
        "FFWOAX-LIFECYCLE-CON.TXT",
        "FFWOAX-LIFECYCLE-COR.TXT",
        "FFWOAX-LIFECYCLE-CAN.TXT",
        "FFWOAX-LIFECYCLE-EXP.TXT",
        "FFWOAX-LIFECYCLE-UPG.TXT",
    ];
    cleanup_rows(&sink, &filenames, &[key]).await;

    let new_timestamp = 1_741_175_200;
    let new_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[0],
            new_timestamp,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "NEW",
                key.etn,
                "250305T1200Z",
                "250305T1800Z",
            )],
        ),
    )
    .await;
    let incident = fetch_incident(&sink, key)
        .await
        .expect("incident should exist after NEW");
    assert_eq!(incident.current_status, "active");
    assert_eq!(incident.latest_vtec_action, "NEW");
    assert_eq!(incident.first_product_id, new_id);
    assert_eq!(incident.latest_product_id, new_id);

    let con_timestamp = new_timestamp + 300;
    let con_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[1],
            con_timestamp,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "CON",
                key.etn,
                "250305T1200Z",
                "250305T1900Z",
            )],
        ),
    )
    .await;
    let incident = fetch_incident(&sink, key)
        .await
        .expect("incident should exist after CON");
    assert_eq!(incident.current_status, "active");
    assert_eq!(incident.latest_vtec_action, "CON");
    assert_eq!(incident.first_product_id, new_id);
    assert_eq!(incident.latest_product_id, con_id);
    assert_eq!(incident.issued_at, utc_timestamp(con_timestamp));
    assert_eq!(
        incident.end_utc,
        Some(
            Utc.with_ymd_and_hms(2025, 3, 5, 19, 0, 0)
                .single()
                .expect("valid timestamp"),
        ),
    );

    let cor_timestamp = con_timestamp + 300;
    let cor_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[2],
            cor_timestamp,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "COR",
                key.etn,
                "250305T1200Z",
                "250305T1900Z",
            )],
        ),
    )
    .await;
    let incident = fetch_incident(&sink, key)
        .await
        .expect("incident should exist after COR");
    assert_eq!(incident.current_status, "active");
    assert_eq!(incident.latest_vtec_action, "COR");
    assert_eq!(incident.first_product_id, new_id);
    assert_eq!(incident.latest_product_id, cor_id);

    let can_timestamp = cor_timestamp + 300;
    let can_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[3],
            can_timestamp,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "CAN",
                key.etn,
                "250305T1200Z",
                "250305T1900Z",
            )],
        ),
    )
    .await;
    let incident = fetch_incident(&sink, key)
        .await
        .expect("incident should exist after CAN");
    assert_eq!(incident.current_status, "cancelled");
    assert_eq!(incident.latest_vtec_action, "CAN");
    assert_eq!(incident.first_product_id, new_id);
    assert_eq!(incident.latest_product_id, can_id);

    let exp_timestamp = can_timestamp + 300;
    let exp_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[4],
            exp_timestamp,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "EXP",
                key.etn,
                "250305T1200Z",
                "250305T1900Z",
            )],
        ),
    )
    .await;
    let incident = fetch_incident(&sink, key)
        .await
        .expect("incident should exist after EXP");
    assert_eq!(incident.current_status, "expired");
    assert_eq!(incident.latest_vtec_action, "EXP");
    assert_eq!(incident.first_product_id, new_id);
    assert_eq!(incident.latest_product_id, exp_id);

    let upg_timestamp = exp_timestamp + 300;
    let upg_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[5],
            upg_timestamp,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "UPG",
                key.etn,
                "250305T1200Z",
                "250305T1900Z",
            )],
        ),
    )
    .await;
    let incident = fetch_incident(&sink, key)
        .await
        .expect("incident should exist after UPG");
    assert_eq!(incident.current_status, "upgraded");
    assert_eq!(incident.latest_vtec_action, "UPG");
    assert_eq!(incident.first_product_id, new_id);
    assert_eq!(incident.latest_product_id, upg_id);
    assert_eq!(
        incident.latest_product_timestamp_utc,
        utc_timestamp(upg_timestamp)
    );

    cleanup_rows(&sink, &filenames, &[key]).await;
}

#[tokio::test]
async fn incident_changes_emit_created_and_updated_notifications() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let key = TestIncidentKey {
        office: "KOAX",
        phenomena: "FF",
        significance: "W",
        etn: 3001,
    };
    let filenames = ["FFWOAX-CHANGE-NEW.TXT", "FFWOAX-CHANGE-CON.TXT"];
    cleanup_rows(&sink, &filenames, &[key]).await;

    let mut rx = sink.subscribe_incident_changes();
    let new_timestamp = 1_741_175_200;
    let new_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[0],
            new_timestamp,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "NEW",
                key.etn,
                "250305T1200Z",
                "250305T1800Z",
            )],
        ),
    )
    .await;
    let created = recv_incident_change(&mut rx).await;
    assert_eq!(created.action, IncidentChangeAction::Created);
    assert_eq!(created.trigger, IncidentChangeTrigger::Persist);
    assert_eq!(created.incident.office, key.office);
    assert_eq!(created.incident.phenomena, key.phenomena);
    assert_eq!(created.incident.significance, key.significance);
    assert_eq!(created.incident.etn, key.etn);
    assert_eq!(created.incident.latest_product_id, new_id);

    let con_timestamp = new_timestamp + 300;
    let con_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[1],
            con_timestamp,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "CON",
                key.etn,
                "250305T1200Z",
                "250305T1900Z",
            )],
        ),
    )
    .await;
    let updated = recv_incident_change(&mut rx).await;
    assert_eq!(updated.action, IncidentChangeAction::Updated);
    assert_eq!(updated.trigger, IncidentChangeTrigger::Persist);
    assert_eq!(updated.incident.latest_product_id, con_id);
    assert_eq!(updated.incident.first_product_id, new_id);
    assert_eq!(updated.incident.latest_vtec_action, "CON");

    cleanup_rows(&sink, &filenames, &[key]).await;
}

#[tokio::test]
async fn incident_projection_collapses_duplicate_keys_within_one_product() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let key = TestIncidentKey {
        office: "KOAX",
        phenomena: "FF",
        significance: "W",
        etn: 2002,
    };
    let filename = "FFWOAX-DUPLICATE-KEYS.TXT";
    cleanup_rows(&sink, &[filename], &[key]).await;

    let product_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filename,
            1_741_176_000,
            "NEC001-051300-",
            &[
                vtec_line('O', "NEW", key.etn, "250305T1215Z", "250305T1700Z"),
                vtec_line('O', "CON", key.etn, "250305T1200Z", "250305T1900Z"),
            ],
        ),
    )
    .await;

    let incident = fetch_incident(&sink, key)
        .await
        .expect("collapsed incident row should exist");
    assert_eq!(incident.current_status, "active");
    assert_eq!(incident.latest_vtec_action, "CON");
    assert_eq!(incident.first_product_id, product_id);
    assert_eq!(incident.latest_product_id, product_id);
    assert_eq!(
        incident.start_utc,
        Some(
            Utc.with_ymd_and_hms(2025, 3, 5, 12, 0, 0)
                .single()
                .expect("valid timestamp"),
        ),
    );
    assert_eq!(
        incident.end_utc,
        Some(
            Utc.with_ymd_and_hms(2025, 3, 5, 19, 0, 0)
                .single()
                .expect("valid timestamp"),
        ),
    );

    let incident_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM incidents WHERE office = $1 AND phenomena = $2 AND significance = $3 AND etn = $4",
    )
    .bind(key.office)
    .bind(key.phenomena)
    .bind(key.significance)
    .bind(key.etn)
    .fetch_one(&sink.pool())
    .await
    .expect("incident count should be queryable");
    assert_eq!(incident_count, 1);

    cleanup_rows(&sink, &[filename], &[key]).await;
}

#[tokio::test]
async fn incident_projection_rejects_stale_updates() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let key = TestIncidentKey {
        office: "KOAX",
        phenomena: "FF",
        significance: "W",
        etn: 2003,
    };
    let filenames = ["FFWOAX-STALE-NEWER.TXT", "FFWOAX-STALE-OLDER.TXT"];
    cleanup_rows(&sink, &filenames, &[key]).await;

    let newer_timestamp = 1_741_176_600;
    let newer_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[0],
            newer_timestamp,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "NEW",
                key.etn,
                "250305T1200Z",
                "250305T1800Z",
            )],
        ),
    )
    .await;

    let older_timestamp = newer_timestamp - 600;
    persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[1],
            older_timestamp,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "CAN",
                key.etn,
                "250305T1200Z",
                "250305T1800Z",
            )],
        ),
    )
    .await;

    let incident = fetch_incident(&sink, key)
        .await
        .expect("incident row should exist after stale replay");
    assert_eq!(incident.current_status, "active");
    assert_eq!(incident.latest_vtec_action, "NEW");
    assert_eq!(incident.latest_product_id, newer_id);
    assert_eq!(
        incident.latest_product_timestamp_utc,
        utc_timestamp(newer_timestamp)
    );

    cleanup_rows(&sink, &filenames, &[key]).await;
}

#[tokio::test]
async fn stale_incident_updates_do_not_emit_notifications() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let key = TestIncidentKey {
        office: "KOAX",
        phenomena: "FF",
        significance: "W",
        etn: 2005,
    };
    let filenames = ["FFWOAX-NOTIFY-NEWER.TXT", "FFWOAX-NOTIFY-OLDER.TXT"];
    cleanup_rows(&sink, &filenames, &[key]).await;

    let mut rx = sink.subscribe_incident_changes();
    let newer_timestamp = 1_741_176_600;
    persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[0],
            newer_timestamp,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "NEW",
                key.etn,
                "250305T1200Z",
                "250305T1800Z",
            )],
        ),
    )
    .await;
    let _ = recv_incident_change(&mut rx).await;

    let older_timestamp = newer_timestamp - 600;
    persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[1],
            older_timestamp,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "CAN",
                key.etn,
                "250305T1200Z",
                "250305T1800Z",
            )],
        ),
    )
    .await;

    let stale_result = timeout(Duration::from_millis(250), rx.recv()).await;
    assert!(
        stale_result.is_err(),
        "stale replay should not emit a change"
    );

    cleanup_rows(&sink, &filenames, &[key]).await;
}

#[tokio::test]
async fn incident_projection_ignores_non_operational_vtec() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let key = TestIncidentKey {
        office: "KOAX",
        phenomena: "FF",
        significance: "W",
        etn: 2004,
    };
    let filename = "FFWOAX-NON-OPERATIONAL.TXT";
    cleanup_rows(&sink, &[filename], &[key]).await;

    let product_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filename,
            1_741_177_200,
            "NEC001-051300-",
            &[vtec_line(
                'T',
                "NEW",
                key.etn,
                "250305T1200Z",
                "250305T1800Z",
            )],
        ),
    )
    .await;

    let product_vtec_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM product_vtec WHERE product_id = $1")
            .bind(product_id)
            .fetch_one(&sink.pool())
            .await
            .expect("product vtec rows should be queryable");
    assert_eq!(product_vtec_count, 1);
    assert!(fetch_incident(&sink, key).await.is_none());

    cleanup_rows(&sink, &[filename], &[key]).await;
}

#[tokio::test]
async fn incident_cleanup_expires_active_rows_with_past_end_utc() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let key = TestIncidentKey {
        office: "KOAX",
        phenomena: "FF",
        significance: "W",
        etn: 2101,
    };
    let filename = "FFWOAX-CLEANUP-PAST-END.TXT";
    cleanup_rows(&sink, &[filename], &[key]).await;

    persist_metadata(
        &sink,
        build_vtec_metadata(
            filename,
            1_741_178_000,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "NEW",
                key.etn,
                "250305T1200Z",
                "250305T1800Z",
            )],
        ),
    )
    .await;

    let cleanup_now = Utc
        .with_ymd_and_hms(2025, 3, 5, 20, 0, 0)
        .single()
        .expect("valid timestamp");
    let result = sink
        .expire_active_incidents(cleanup_now)
        .await
        .expect("cleanup should succeed");
    assert_eq!(result.expired_count, 1);

    let incident = fetch_incident(&sink, key)
        .await
        .expect("incident should remain present after cleanup");
    assert_eq!(incident.current_status, "expired");

    cleanup_rows(&sink, &[filename], &[key]).await;
}

#[tokio::test]
async fn incident_cleanup_emits_update_notifications() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let key = TestIncidentKey {
        office: "KOAX",
        phenomena: "FF",
        significance: "W",
        etn: 2105,
    };
    let filename = "FFWOAX-CLEANUP-NOTIFY.TXT";
    cleanup_rows(&sink, &[filename], &[key]).await;

    let mut rx = sink.subscribe_incident_changes();
    persist_metadata(
        &sink,
        build_vtec_metadata(
            filename,
            1_741_178_000,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "NEW",
                key.etn,
                "250305T1200Z",
                "250305T1800Z",
            )],
        ),
    )
    .await;
    let _ = recv_incident_change(&mut rx).await;

    let cleanup_now = Utc
        .with_ymd_and_hms(2025, 3, 5, 20, 0, 0)
        .single()
        .expect("valid timestamp");
    sink.expire_active_incidents(cleanup_now)
        .await
        .expect("cleanup should succeed");

    let change = recv_incident_change(&mut rx).await;
    assert_eq!(change.action, IncidentChangeAction::Updated);
    assert_eq!(change.trigger, IncidentChangeTrigger::Cleanup);
    assert_eq!(change.incident.current_status, "expired");
    assert_eq!(change.incident.etn, key.etn);

    cleanup_rows(&sink, &[filename], &[key]).await;
}

#[tokio::test]
async fn incident_cleanup_skips_future_end_utc_and_null_end_utc() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let future_key = TestIncidentKey {
        office: "KOAX",
        phenomena: "FF",
        significance: "W",
        etn: 2102,
    };
    let null_key = TestIncidentKey {
        office: "KOAX",
        phenomena: "FF",
        significance: "W",
        etn: 2103,
    };
    let filenames = ["FFWOAX-CLEANUP-FUTURE.TXT", "FFWOAX-CLEANUP-NULL.TXT"];
    cleanup_rows(&sink, &filenames, &[future_key, null_key]).await;

    persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[0],
            1_741_178_300,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "NEW",
                future_key.etn,
                "250305T1200Z",
                "250305T2100Z",
            )],
        ),
    )
    .await;
    persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[1],
            1_741_178_400,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "NEW",
                null_key.etn,
                "250305T1200Z",
                "250305T1800Z",
            )],
        ),
    )
    .await;
    update_incident_end_utc(&sink, null_key, None).await;

    let cleanup_now = Utc
        .with_ymd_and_hms(2025, 3, 5, 20, 0, 0)
        .single()
        .expect("valid timestamp");
    let result = sink
        .expire_active_incidents(cleanup_now)
        .await
        .expect("cleanup should succeed");
    assert_eq!(result.expired_count, 0);

    let future_incident = fetch_incident(&sink, future_key)
        .await
        .expect("future incident should still exist");
    assert_eq!(future_incident.current_status, "active");
    let null_incident = fetch_incident(&sink, null_key)
        .await
        .expect("null-end incident should still exist");
    assert_eq!(null_incident.current_status, "active");
    assert_eq!(null_incident.end_utc, None);

    cleanup_rows(&sink, &filenames, &[future_key, null_key]).await;
}

#[tokio::test]
async fn incident_cleanup_skips_non_active_rows() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let key = TestIncidentKey {
        office: "KOAX",
        phenomena: "FF",
        significance: "W",
        etn: 2104,
    };
    let filename = "FFWOAX-CLEANUP-NON-ACTIVE.TXT";
    cleanup_rows(&sink, &[filename], &[key]).await;

    persist_metadata(
        &sink,
        build_vtec_metadata(
            filename,
            1_741_178_600,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "NEW",
                key.etn,
                "250305T1200Z",
                "250305T1800Z",
            )],
        ),
    )
    .await;
    update_incident_status(&sink, key, "cancelled").await;

    let cleanup_now = Utc
        .with_ymd_and_hms(2025, 3, 5, 20, 0, 0)
        .single()
        .expect("valid timestamp");
    let result = sink
        .expire_active_incidents(cleanup_now)
        .await
        .expect("cleanup should succeed");
    assert_eq!(result.expired_count, 0);

    let incident = fetch_incident(&sink, key)
        .await
        .expect("cancelled incident should still exist");
    assert_eq!(incident.current_status, "cancelled");

    cleanup_rows(&sink, &[filename], &[key]).await;
}

#[tokio::test]
async fn incident_cleanup_preserves_latest_product_timestamp_and_latest_vtec_action() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let key = TestIncidentKey {
        office: "KOAX",
        phenomena: "FF",
        significance: "W",
        etn: 2105,
    };
    let filenames = [
        "FFWOAX-CLEANUP-PRESERVE-NEW.TXT",
        "FFWOAX-CLEANUP-PRESERVE-CON.TXT",
    ];
    cleanup_rows(&sink, &filenames, &[key]).await;

    let first_product_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[0],
            1_741_178_900,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "NEW",
                key.etn,
                "250305T1200Z",
                "250305T1800Z",
            )],
        ),
    )
    .await;
    let latest_timestamp = 1_741_179_200;
    let latest_product_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[1],
            latest_timestamp,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "CON",
                key.etn,
                "250305T1200Z",
                "250305T1800Z",
            )],
        ),
    )
    .await;

    let before = fetch_incident(&sink, key)
        .await
        .expect("incident should exist before cleanup");
    let cleanup_now = Utc
        .with_ymd_and_hms(2025, 3, 5, 20, 0, 0)
        .single()
        .expect("valid timestamp");
    let result = sink
        .expire_active_incidents(cleanup_now)
        .await
        .expect("cleanup should succeed");
    assert_eq!(result.expired_count, 1);

    let after = fetch_incident(&sink, key)
        .await
        .expect("incident should still exist after cleanup");
    assert_eq!(after.current_status, "expired");
    assert_eq!(after.latest_vtec_action, before.latest_vtec_action);
    assert_eq!(after.latest_vtec_action, "CON");
    assert_eq!(after.first_product_id, first_product_id);
    assert_eq!(after.latest_product_id, latest_product_id);
    assert_eq!(
        after.latest_product_timestamp_utc,
        before.latest_product_timestamp_utc
    );
    assert_eq!(
        after.latest_product_timestamp_utc,
        utc_timestamp(latest_timestamp)
    );
    assert_eq!(after.issued_at, before.issued_at);

    cleanup_rows(&sink, &filenames, &[key]).await;
}

#[tokio::test]
async fn archive_read_queries_return_incident_and_product_views() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let key = TestIncidentKey {
        office: "KOAX",
        phenomena: "FF",
        significance: "W",
        etn: 3101,
    };
    let filenames = ["FFWOAX-READ-1.TXT", "FFWOAX-READ-2.TXT"];
    cleanup_rows(&sink, &filenames, &[key]).await;

    let first_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[0],
            1_741_180_000,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "NEW",
                key.etn,
                "250305T1200Z",
                "250305T1800Z",
            )],
        ),
    )
    .await;
    let second_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[1],
            1_741_180_300,
            "NEC001-051300-",
            &[vtec_line(
                'O',
                "CON",
                key.etn,
                "250305T1200Z",
                "250305T1900Z",
            )],
        ),
    )
    .await;

    let incidents = sink
        .list_incidents(emwin_db::IncidentListQuery {
            office: Some("KOAX".to_string()),
            limit: 10,
            ..Default::default()
        })
        .await
        .expect("incident list query should succeed");
    assert_eq!(incidents.items.len(), 1);
    assert_eq!(incidents.items[0].latest_product_id, second_id);

    let incident = sink
        .get_incident(&emwin_db::IncidentKey {
            office: key.office.to_string(),
            phenomena: key.phenomena.to_string(),
            significance: key.significance.to_string(),
            etn: key.etn,
        })
        .await
        .expect("incident detail query should succeed")
        .expect("incident should exist");
    assert_eq!(incident.first_product_id, first_id);
    assert_eq!(incident.latest_product_id, second_id);

    let products = sink
        .list_incident_products(
            &emwin_db::IncidentKey {
                office: key.office.to_string(),
                phenomena: key.phenomena.to_string(),
                significance: key.significance.to_string(),
                etn: key.etn,
            },
            emwin_db::IncidentProductsQuery::default(),
        )
        .await
        .expect("incident products query should succeed");
    assert_eq!(products.items.len(), 2);
    assert_eq!(products.items[0].product_id, first_id);
    assert_eq!(products.items[1].product_id, second_id);

    let product = sink
        .get_archived_product(second_id)
        .await
        .expect("archived product query should succeed")
        .expect("product should exist");
    assert_eq!(product.summary.product_id, second_id);
    assert_eq!(product.summary.filename, filenames[1]);

    cleanup_rows(&sink, &filenames, &[key]).await;
}

#[tokio::test]
async fn incident_list_pagination_uses_last_returned_item_for_cursor() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let keys = [
        TestIncidentKey {
            office: "KOAX",
            phenomena: "FF",
            significance: "W",
            etn: 3201,
        },
        TestIncidentKey {
            office: "KOAX",
            phenomena: "FF",
            significance: "W",
            etn: 3202,
        },
        TestIncidentKey {
            office: "KOAX",
            phenomena: "FF",
            significance: "W",
            etn: 3203,
        },
    ];
    let filenames = [
        "FFWOAX-PAGE-1.TXT",
        "FFWOAX-PAGE-2.TXT",
        "FFWOAX-PAGE-3.TXT",
    ];
    cleanup_rows(&sink, &filenames, &keys).await;

    let expected_order = [keys[2].etn, keys[1].etn, keys[0].etn];
    for (index, (key, filename)) in keys.into_iter().zip(filenames).enumerate() {
        persist_metadata(
            &sink,
            build_vtec_metadata(
                filename,
                1_741_180_000 + u64::try_from(index).expect("index should fit") * 300,
                "NEC001-051300-",
                &[vtec_line(
                    'O',
                    "NEW",
                    key.etn,
                    "250305T1200Z",
                    "250305T1800Z",
                )],
            ),
        )
        .await;
    }

    let first_page = sink
        .list_incidents(emwin_db::IncidentListQuery {
            office: Some("KOAX".to_string()),
            limit: 2,
            ..Default::default()
        })
        .await
        .expect("first incident page should succeed");
    assert_eq!(
        first_page
            .items
            .iter()
            .map(|item| item.etn)
            .collect::<Vec<_>>(),
        expected_order[..2]
    );
    assert!(
        first_page.next_cursor.is_some(),
        "first page should expose a continuation cursor"
    );

    let second_page = sink
        .list_incidents(emwin_db::IncidentListQuery {
            office: Some("KOAX".to_string()),
            limit: 2,
            cursor: first_page.next_cursor,
            ..Default::default()
        })
        .await
        .expect("second incident page should succeed");
    assert_eq!(
        second_page
            .items
            .iter()
            .map(|item| item.etn)
            .collect::<Vec<_>>(),
        expected_order[2..]
    );
    assert_eq!(second_page.next_cursor, None);

    cleanup_rows(&sink, &filenames, &keys).await;
}

#[tokio::test]
async fn incident_products_pagination_uses_last_returned_item_for_cursor() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let key = TestIncidentKey {
        office: "KOAX",
        phenomena: "FF",
        significance: "W",
        etn: 3301,
    };
    let filenames = [
        "FFWOAX-PRODUCT-PAGE-1.TXT",
        "FFWOAX-PRODUCT-PAGE-2.TXT",
        "FFWOAX-PRODUCT-PAGE-3.TXT",
    ];
    cleanup_rows(&sink, &filenames, &[key]).await;

    let mut expected_product_ids = Vec::new();
    for (index, filename) in filenames.into_iter().enumerate() {
        let product_id = persist_metadata(
            &sink,
            build_vtec_metadata(
                filename,
                1_741_181_000 + u64::try_from(index).expect("index should fit") * 300,
                "NEC001-051300-",
                &[vtec_line(
                    'O',
                    "NEW",
                    key.etn,
                    "250305T1200Z",
                    "250305T1800Z",
                )],
            ),
        )
        .await;
        expected_product_ids.push(product_id);
    }

    let first_page = sink
        .list_incident_products(
            &emwin_db::IncidentKey {
                office: key.office.to_string(),
                phenomena: key.phenomena.to_string(),
                significance: key.significance.to_string(),
                etn: key.etn,
            },
            emwin_db::IncidentProductsQuery {
                limit: 2,
                ..Default::default()
            },
        )
        .await
        .expect("first incident products page should succeed");
    assert_eq!(
        first_page
            .items
            .iter()
            .map(|item| item.product_id)
            .collect::<Vec<_>>(),
        expected_product_ids[..2]
    );
    assert!(
        first_page.next_cursor.is_some(),
        "first page should expose a continuation cursor"
    );

    let second_page = sink
        .list_incident_products(
            &emwin_db::IncidentKey {
                office: key.office.to_string(),
                phenomena: key.phenomena.to_string(),
                significance: key.significance.to_string(),
                etn: key.etn,
            },
            emwin_db::IncidentProductsQuery {
                limit: 2,
                cursor: first_page.next_cursor,
            },
        )
        .await
        .expect("second incident products page should succeed");
    assert_eq!(
        second_page
            .items
            .iter()
            .map(|item| item.product_id)
            .collect::<Vec<_>>(),
        expected_product_ids[2..]
    );
    assert_eq!(second_page.next_cursor, None);

    cleanup_rows(&sink, &filenames, &[key]).await;
}

#[tokio::test]
async fn archive_payload_reads_filesystem_backed_bytes() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let temp = tempfile::tempdir().expect("tempdir should exist");
    let payload_path = temp.path().join("payload.txt");
    let sidecar_path = temp.path().join("payload.JSON");
    std::fs::write(&payload_path, b"raw archive body").expect("payload write should succeed");
    std::fs::write(&sidecar_path, b"{}").expect("sidecar write should succeed");

    let metadata = sample_metadata();
    let incident_key = TestIncidentKey {
        office: "KOAX",
        phenomena: "FF",
        significance: "W",
        etn: 1,
    };
    cleanup_rows(&sink, &[&metadata.filename], &[incident_key]).await;

    let product_id = persist_metadata_with_blobs(
        &sink,
        metadata.clone(),
        sample_filesystem_blobs_at(
            &payload_path.to_string_lossy(),
            &sidecar_path.to_string_lossy(),
        ),
    )
    .await;

    let payload = sink
        .read_archived_payload(product_id)
        .await
        .expect("raw payload read should succeed")
        .expect("raw payload should exist");
    assert_eq!(payload.filename, metadata.filename);
    assert_eq!(payload.bytes, b"raw archive body");

    cleanup_rows(&sink, &[&metadata.filename], &[incident_key]).await;
}

#[tokio::test]
async fn archive_payload_reads_writer_locations_from_relative_filesystem_roots() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let temp = tempfile::Builder::new()
        .prefix("emwin-db-archive-relative-")
        .tempdir_in(".")
        .expect("tempdir in cwd should exist");
    let root = std::path::PathBuf::from(
        temp.path()
            .file_name()
            .expect("tempdir should have a file name"),
    );
    let writer = emwin_db::FilesystemBlobWriter::new(root);
    let payload_entry = emwin_db::BlobEntry::new(
        BlobRole::Payload,
        "archive/payload.txt",
        b"raw archive body".to_vec(),
        Some("text/plain"),
    );
    let sidecar_entry = emwin_db::BlobEntry::new(
        BlobRole::MetadataSidecar,
        "archive/payload.JSON",
        b"{}".to_vec(),
        Some("application/json"),
    );
    let payload_blob = writer
        .write(&payload_entry)
        .await
        .expect("payload write should succeed");
    let sidecar_blob = writer
        .write(&sidecar_entry)
        .await
        .expect("sidecar write should succeed");
    assert!(
        std::path::Path::new(&payload_blob.location).is_absolute(),
        "relative filesystem roots should persist absolute payload locations"
    );

    let metadata = sample_metadata();
    let incident_key = TestIncidentKey {
        office: "KOAX",
        phenomena: "FF",
        significance: "W",
        etn: 2,
    };
    cleanup_rows(&sink, &[&metadata.filename], &[incident_key]).await;

    let product_id =
        persist_metadata_with_blobs(&sink, metadata.clone(), vec![payload_blob, sidecar_blob])
            .await;

    let payload = sink
        .read_archived_payload(product_id)
        .await
        .expect("raw payload read should succeed")
        .expect("raw payload should exist");
    assert_eq!(payload.filename, metadata.filename);
    assert_eq!(payload.bytes, b"raw archive body");

    cleanup_rows(&sink, &[&metadata.filename], &[incident_key]).await;
}
