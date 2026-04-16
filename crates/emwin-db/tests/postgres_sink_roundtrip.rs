mod common;

use chrono::{DateTime, Utc};
use common::*;
use sqlx::Row;

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
