mod common;

use chrono::TimeZone;
use common::*;

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
async fn archived_product_list_supports_filters_and_cursor_pagination() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let filenames = [
        "FFWOAX-PRODUCT-LIST-1.TXT",
        "FFWOAX-PRODUCT-LIST-2.TXT",
        "FFWGSP-PRODUCT-LIST-3.TXT",
    ];
    cleanup_rows(&sink, &filenames, &[]).await;

    let mut product_ids = Vec::new();
    product_ids.push(
        persist_metadata(
            &sink,
            build_vtec_metadata(
                filenames[0],
                1_741_182_000,
                "NEC001-051300-",
                &[vtec_line('O', "NEW", 3401, "250305T1200Z", "250305T1800Z")],
            ),
        )
        .await,
    );
    product_ids.push(
        persist_metadata(
            &sink,
            build_vtec_metadata(
                filenames[1],
                1_741_182_300,
                "NEC001-051300-",
                &[vtec_line('O', "CON", 3401, "250305T1200Z", "250305T1900Z")],
            ),
        )
        .await,
    );
    product_ids.push(
        persist_metadata(
            &sink,
            emwin_db::CompletedFileMetadata::build(
                filenames[2],
                1_741_182_600,
                emwin_protocol::ingest::ProductOrigin::Qbt,
                b"000\nWUUS52 KGSP 051200\nFFWGSP\n\nFlash Flood Warning\nNational Weather Service Greenville-Spartanburg SC\n1200 PM EST Wed Mar 5 2025\n\nSCC045-051300-\n/O.NEW.KGSP.FF.W.0001.250305T1200Z-250305T1800Z/\n",
            ),
        )
        .await,
    );

    let first_page = sink
        .list_archived_products(emwin_db::ProductListQuery {
            office: Some("KOAX".to_string()),
            artifact_kind: Some("nws_text_product".to_string()),
            limit: 1,
            ..Default::default()
        })
        .await
        .expect("first product page should succeed");
    assert_eq!(first_page.items.len(), 1);
    assert_eq!(first_page.items[0].product_id, product_ids[1]);
    assert_eq!(first_page.items[0].office_code.as_deref(), Some("KOAX"));
    assert!(first_page.next_cursor.is_some());

    let second_page = sink
        .list_archived_products(emwin_db::ProductListQuery {
            office: Some("KOAX".to_string()),
            artifact_kind: Some("nws_text_product".to_string()),
            limit: 1,
            cursor: first_page.next_cursor,
            ..Default::default()
        })
        .await
        .expect("second product page should succeed");
    assert_eq!(second_page.items.len(), 1);
    assert_eq!(second_page.items[0].product_id, product_ids[0]);
    assert_eq!(second_page.next_cursor, None);

    let issue_filtered = sink
        .list_archived_products(emwin_db::ProductListQuery {
            office: Some("KGSP".to_string()),
            artifact_kind: Some("nws_text_product".to_string()),
            vtec_action: Some("NEW".to_string()),
            state: Some("SC".to_string()),
            limit: 10,
            ..Default::default()
        })
        .await
        .expect("filtered product list should succeed");
    assert_eq!(issue_filtered.items.len(), 1);
    assert_eq!(issue_filtered.items[0].product_id, product_ids[2]);

    cleanup_rows(&sink, &filenames, &[]).await;
}

#[tokio::test]
async fn archived_product_list_supports_bounding_box_filters() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let filenames = ["BBOX-KOAX-1.TXT", "BBOX-KGSP-2.TXT"];
    cleanup_rows(&sink, &filenames, &[]).await;

    let inside_id = persist_metadata(
        &sink,
        emwin_db::CompletedFileMetadata::build(
            filenames[0],
            1_741_182_900,
            emwin_protocol::ingest::ProductOrigin::Qbt,
            br#"000
WUUS53 KOAX 051200
SVROAX

Severe Thunderstorm Warning
National Weather Service Omaha/Valley NE
1200 PM CST Wed Mar 5 2025

NEC001-051300-
/O.NEW.KOAX.SV.W.0001.250305T1200Z-250305T1800Z/

LAT...LON 4143 9613 4145 9610 4140 9608 4138 9612
"#,
        ),
    )
    .await;
    let outside_id = persist_metadata(
        &sink,
        emwin_db::CompletedFileMetadata::build(
            filenames[1],
            1_741_183_200,
            emwin_protocol::ingest::ProductOrigin::Qbt,
            br#"000
WUUS52 KGSP 051200
SVRGSP

Severe Thunderstorm Warning
National Weather Service Greenville-Spartanburg SC
1200 PM EST Wed Mar 5 2025

SCC045-051300-
/O.NEW.KGSP.SV.W.0001.250305T1200Z-250305T1800Z/

LAT...LON 3500 8200 3502 8198 3501 8195 3498 8197
"#,
        ),
    )
    .await;

    let filtered = sink
        .list_archived_products(emwin_db::ProductListQuery {
            min_lat: Some(41.0),
            max_lat: Some(42.0),
            min_lon: Some(-97.0),
            max_lon: Some(-95.0),
            limit: 10,
            ..Default::default()
        })
        .await
        .expect("bbox product list should succeed");
    assert_eq!(filtered.items.len(), 1);
    assert_eq!(filtered.items[0].product_id, inside_id);
    assert_ne!(filtered.items[0].product_id, outside_id);

    cleanup_rows(&sink, &filenames, &[]).await;
}

#[tokio::test]
async fn archived_product_list_matches_office_city_case_insensitively() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let filenames = ["CITY-KOAX-1.TXT"];
    cleanup_rows(&sink, &filenames, &[]).await;

    let product_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[0],
            1_741_183_500,
            "NEC001-051300-",
            &[vtec_line('O', "NEW", 3450, "250305T1200Z", "250305T1800Z")],
        ),
    )
    .await;

    let filtered = sink
        .list_archived_products(emwin_db::ProductListQuery {
            office_city: Some("omaha/valley".to_string()),
            ..Default::default()
        })
        .await
        .expect("office city filter should succeed");
    assert_eq!(filtered.items.len(), 1);
    assert_eq!(filtered.items[0].product_id, product_id);

    cleanup_rows(&sink, &filenames, &[]).await;
}

#[tokio::test]
async fn archive_query_defaults_match_workspace_pagination_conventions() {
    assert_eq!(emwin_db::ProductListQuery::default().limit, 100);
    assert_eq!(emwin_db::FeatureListQuery::default().filters.limit, 100);
}

#[tokio::test]
async fn archived_features_support_pagination_and_kind_filtering() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let filenames = ["FEATURES-KOAX-1.TXT", "FEATURES-KOAX-2.TXT"];
    cleanup_rows(&sink, &filenames, &[]).await;

    let first_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[0],
            1_741_182_000,
            "NEC001-051300-",
            &[vtec_line('O', "NEW", 3501, "250305T1200Z", "250305T1800Z")],
        ),
    )
    .await;
    let second_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[1],
            1_741_182_300,
            "NEC001-051300-",
            &[vtec_line('O', "NEW", 3502, "250305T1200Z", "250305T1800Z")],
        ),
    )
    .await;

    sqlx::query(
        "INSERT INTO product_polygons (product_id, segment_index, polygon_index, polygon_wkt, polygon_geom)
         VALUES ($1, 0, 0, 'POLYGON((-96 41,-95 41,-95 42,-96 42,-96 41))', ST_GeomFromText('POLYGON((-96 41,-95 41,-95 42,-96 42,-96 41))', 4326))",
    )
    .bind(first_id)
    .execute(&sink.pool())
    .await
    .expect("polygon insert should succeed");
    sqlx::query(
        "INSERT INTO product_search_points (product_id, source_kind, source_index, latitude, longitude, point_geom)
         VALUES ($1, 'manual', 0, 41.5, -95.5, ST_SetSRID(ST_MakePoint(-95.5, 41.5), 4326))",
    )
    .bind(first_id)
    .execute(&sink.pool())
    .await
    .expect("search point insert should succeed");
    sqlx::query(
        "INSERT INTO product_search_points (product_id, source_kind, source_index, latitude, longitude, point_geom)
         VALUES ($1, 'manual', 1, 35.0, -82.0, ST_SetSRID(ST_MakePoint(-82.0, 35.0), 4326))",
    )
    .bind(second_id)
    .execute(&sink.pool())
    .await
    .expect("second search point insert should succeed");

    let first_page = sink
        .list_archived_features(emwin_db::FeatureListQuery {
            filters: emwin_db::ProductListQuery {
                office: Some("KOAX".to_string()),
                artifact_kind: Some("nws_text_product".to_string()),
                limit: 2,
                ..Default::default()
            },
            kind: None,
        })
        .await
        .expect("feature page should succeed");
    assert_eq!(first_page.items.len(), 2);
    assert_eq!(first_page.items[0].product_id, second_id);
    assert!(first_page.next_cursor.is_some());

    let second_page = sink
        .list_archived_features(emwin_db::FeatureListQuery {
            filters: emwin_db::ProductListQuery {
                office: Some("KOAX".to_string()),
                artifact_kind: Some("nws_text_product".to_string()),
                limit: 2,
                cursor: first_page.next_cursor,
                ..Default::default()
            },
            kind: None,
        })
        .await
        .expect("second feature page should succeed");
    assert!(
        second_page
            .items
            .iter()
            .all(|item| item.product_id == first_id)
    );
    assert!(
        second_page
            .items
            .iter()
            .any(|item| item.feature_kind == emwin_db::FeatureKind::Polygon)
    );

    let search_points = sink
        .list_archived_features(emwin_db::FeatureListQuery {
            filters: emwin_db::ProductListQuery {
                office: Some("KOAX".to_string()),
                artifact_kind: Some("nws_text_product".to_string()),
                limit: 10,
                ..Default::default()
            },
            kind: Some(emwin_db::FeatureKind::SearchPoint),
        })
        .await
        .expect("filtered features should succeed");
    assert_eq!(search_points.items.len(), 2);
    assert!(
        search_points
            .items
            .iter()
            .all(|item| item.feature_kind == emwin_db::FeatureKind::SearchPoint)
    );

    cleanup_rows(&sink, &filenames, &[]).await;
}

#[tokio::test]
async fn archived_features_bbox_filters_only_return_matching_geometries() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let filenames = ["FEATURE-BBOX-SAME-PRODUCT.TXT"];
    cleanup_rows(&sink, &filenames, &[]).await;

    let product_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[0],
            1_741_182_450,
            "NEC001-051300-",
            &[vtec_line('O', "NEW", 3550, "250305T1200Z", "250305T1800Z")],
        ),
    )
    .await;

    sqlx::query(
        "INSERT INTO product_search_points (product_id, source_kind, source_index, latitude, longitude, point_geom)
         VALUES
            ($1, 'manual', 0, 41.5, -95.5, ST_SetSRID(ST_MakePoint(-95.5, 41.5), 4326)),
            ($1, 'manual', 1, 35.0, -82.0, ST_SetSRID(ST_MakePoint(-82.0, 35.0), 4326))",
    )
    .bind(product_id)
    .execute(&sink.pool())
    .await
    .expect("search point inserts should succeed");

    let filtered = sink
        .list_archived_features(emwin_db::FeatureListQuery {
            filters: emwin_db::ProductListQuery {
                min_lat: Some(41.0),
                max_lat: Some(42.0),
                min_lon: Some(-97.0),
                max_lon: Some(-95.0),
                limit: 10,
                ..Default::default()
            },
            kind: Some(emwin_db::FeatureKind::SearchPoint),
        })
        .await
        .expect("bbox filtered features should succeed");

    assert_eq!(filtered.items.len(), 1);
    assert_eq!(filtered.items[0].product_id, product_id);
    assert_eq!(
        filtered.items[0].feature_kind,
        emwin_db::FeatureKind::SearchPoint
    );

    cleanup_rows(&sink, &filenames, &[]).await;
}

#[tokio::test]
async fn archived_features_point_radius_requires_polygon_containment() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let filenames = ["FEATURE-RADIUS-SEMANTICS.TXT"];
    cleanup_rows(&sink, &filenames, &[]).await;

    let product_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[0],
            1_741_182_460,
            "NEC001-051300-",
            &[vtec_line('O', "NEW", 3551, "250305T1200Z", "250305T1800Z")],
        ),
    )
    .await;

    sqlx::query(
        "INSERT INTO product_polygons (product_id, segment_index, polygon_index, polygon_wkt, polygon_geom)
         VALUES ($1, 0, 0, 'POLYGON((-96.04 41.50,-96.02 41.50,-96.02 41.52,-96.04 41.52,-96.04 41.50))',
                 ST_GeomFromText('POLYGON((-96.04 41.50,-96.02 41.50,-96.02 41.52,-96.04 41.52,-96.04 41.50))', 4326))",
    )
    .bind(product_id)
    .execute(&sink.pool())
    .await
    .expect("polygon insert should succeed");
    sqlx::query(
        "INSERT INTO product_search_points (product_id, source_kind, source_index, latitude, longitude, point_geom)
         VALUES ($1, 'manual', 0, 41.50, -95.99, ST_SetSRID(ST_MakePoint(-95.99, 41.50), 4326))",
    )
    .bind(product_id)
    .execute(&sink.pool())
    .await
    .expect("search point insert should succeed");

    let filtered = sink
        .list_archived_features(emwin_db::FeatureListQuery {
            filters: emwin_db::ProductListQuery {
                lat: Some(41.50),
                lon: Some(-95.99),
                distance_miles: Some(5.0),
                ..Default::default()
            },
            kind: None,
        })
        .await
        .expect("radius feature query should succeed");

    assert!(
        filtered
            .items
            .iter()
            .any(|item| item.feature_kind == emwin_db::FeatureKind::SearchPoint),
        "nearby point feature should match"
    );
    assert!(
        filtered
            .items
            .iter()
            .all(|item| item.feature_kind != emwin_db::FeatureKind::Polygon),
        "polygon should not match when point lies outside it"
    );

    cleanup_rows(&sink, &filenames, &[]).await;
}

#[tokio::test]
async fn archived_aggregate_queries_cover_facets_timeseries_and_cells() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let filenames = ["AGG-KOAX-1.TXT", "AGG-KOAX-2.TXT"];
    cleanup_rows(&sink, &filenames, &[]).await;

    let first_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[0],
            1_741_182_000,
            "NEC001-051300-",
            &[vtec_line('O', "NEW", 3601, "250305T1200Z", "250305T1800Z")],
        ),
    )
    .await;
    let second_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[1],
            1_741_185_900,
            "NEC001-051300-",
            &[vtec_line('O', "NEW", 3602, "250305T1200Z", "250305T1800Z")],
        ),
    )
    .await;

    for product_id in [first_id, second_id] {
        sqlx::query(
            "INSERT INTO product_issues (product_id, kind, code, message, line)
             VALUES ($1, 'text_product_parse', 'invalid_wmo_header', 'failed', NULL)",
        )
        .bind(product_id)
        .execute(&sink.pool())
        .await
        .expect("issue insert should succeed");
    }
    sqlx::query(
        "INSERT INTO product_polygons (product_id, segment_index, polygon_index, polygon_wkt, polygon_geom)
         VALUES ($1, 0, 0, 'POLYGON((-96.0 41.4,-95.8 41.4,-95.8 41.6,-96.0 41.6,-96.0 41.4))',
                 ST_GeomFromText('POLYGON((-96.0 41.4,-95.8 41.4,-95.8 41.6,-96.0 41.6,-96.0 41.4))', 4326))",
    )
    .bind(first_id)
    .execute(&sink.pool())
    .await
    .expect("polygon insert should succeed");
    sqlx::query(
        "INSERT INTO product_time_mot_loc (product_id, segment_index, entry_index, time_utc, direction_degrees, speed_kt, path_wkt, path_geom)
         VALUES ($1, 0, 0, TIMESTAMPTZ '2025-03-05T13:00:00Z', 300, 25,
                 'LINESTRING(-95.9 41.5, -95.7 41.7)',
                 ST_GeomFromText('LINESTRING(-95.9 41.5, -95.7 41.7)', 4326))",
    )
    .bind(second_id)
    .execute(&sink.pool())
    .await
    .expect("path insert should succeed");

    let issue_facets = sink
        .list_facet_aggregate(emwin_db::FacetAggregateQuery {
            filters: emwin_db::ProductListQuery {
                office: Some("KOAX".to_string()),
                artifact_kind: Some("nws_text_product".to_string()),
                limit: 100,
                ..Default::default()
            },
            dimension: emwin_db::FacetDimension::IssueKind,
            limit: 20,
        })
        .await
        .expect("facet query should succeed");
    assert!(!issue_facets.completeness.partial);
    assert!(!issue_facets.completeness.approximate);
    assert_eq!(issue_facets.items[0].value, "text_product_parse");
    assert_eq!(issue_facets.items[0].count, 2);

    let timeseries = sink
        .list_timeseries_aggregate(emwin_db::TimeseriesAggregateQuery {
            filters: emwin_db::ProductListQuery {
                office: Some("KOAX".to_string()),
                artifact_kind: Some("nws_text_product".to_string()),
                limit: 100,
                ..Default::default()
            },
            measure: emwin_db::TimeseriesMeasure::ProductCount,
            start: chrono::Utc
                .timestamp_opt(1_741_181_800, 0)
                .single()
                .expect("valid start"),
            end: chrono::Utc
                .timestamp_opt(1_741_189_000, 0)
                .single()
                .expect("valid end"),
            bucket: emwin_db::TimeseriesBucket::Hour,
        })
        .await
        .expect("timeseries query should succeed");
    assert!(!timeseries.completeness.partial);
    assert_eq!(timeseries.items.len(), 2);
    assert_eq!(timeseries.items[0].count, 1);
    assert_eq!(timeseries.items[1].count, 1);

    let cells = sink
        .list_cell_aggregate(emwin_db::CellAggregateQuery {
            filters: emwin_db::ProductListQuery {
                office: Some("KOAX".to_string()),
                artifact_kind: Some("nws_text_product".to_string()),
                limit: 100,
                ..Default::default()
            },
            measure: emwin_db::CellMeasure::ProductCount,
            precision: 5,
            limit: 10,
        })
        .await
        .expect("cell query should succeed");
    assert!(!cells.completeness.partial);
    assert!(!cells.items.is_empty());
    assert!(cells.items.iter().map(|item| item.count).sum::<i64>() >= 2);

    cleanup_rows(&sink, &filenames, &[]).await;
}

#[tokio::test]
async fn archived_cell_aggregate_counts_only_matching_geometries() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let filenames = ["CELL-BBOX-SAME-PRODUCT.TXT"];
    cleanup_rows(&sink, &filenames, &[]).await;

    let product_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[0],
            1_741_182_550,
            "NEC001-051300-",
            &[vtec_line('O', "NEW", 3650, "250305T1200Z", "250305T1800Z")],
        ),
    )
    .await;

    sqlx::query(
        "INSERT INTO product_polygons (product_id, segment_index, polygon_index, polygon_wkt, polygon_geom)
         VALUES ($1, 0, 0, 'POLYGON((-96.0 41.4,-95.8 41.4,-95.8 41.6,-96.0 41.6,-96.0 41.4))',
                 ST_GeomFromText('POLYGON((-96.0 41.4,-95.8 41.4,-95.8 41.6,-96.0 41.6,-96.0 41.4))', 4326))",
    )
    .bind(product_id)
    .execute(&sink.pool())
    .await
    .expect("polygon insert should succeed");
    sqlx::query(
        "INSERT INTO product_search_points (product_id, source_kind, source_index, latitude, longitude, point_geom)
         VALUES ($1, 'manual', 1, 35.0, -82.0, ST_SetSRID(ST_MakePoint(-82.0, 35.0), 4326))",
    )
    .bind(product_id)
    .execute(&sink.pool())
    .await
    .expect("search point insert should succeed");

    let cells = sink
        .list_cell_aggregate(emwin_db::CellAggregateQuery {
            filters: emwin_db::ProductListQuery {
                min_lat: Some(41.0),
                max_lat: Some(42.0),
                min_lon: Some(-97.0),
                max_lon: Some(-95.0),
                limit: 100,
                ..Default::default()
            },
            measure: emwin_db::CellMeasure::ProductCount,
            precision: 5,
            limit: 10,
        })
        .await
        .expect("cell aggregate should succeed");

    assert_eq!(cells.items.len(), 1);
    assert_eq!(cells.items[0].count, 1);

    cleanup_rows(&sink, &filenames, &[]).await;
}

#[tokio::test]
async fn archived_cell_aggregate_counts_polygon_and_path_products() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let filenames = ["CELL-POLYGON-ONLY.TXT", "CELL-PATH-ONLY.TXT"];
    cleanup_rows(&sink, &filenames, &[]).await;

    let polygon_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[0],
            1_741_182_560,
            "NEC001-051300-",
            &[vtec_line('O', "NEW", 3651, "250305T1200Z", "250305T1800Z")],
        ),
    )
    .await;
    let path_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[1],
            1_741_182_860,
            "NEC001-051300-",
            &[vtec_line('O', "NEW", 3652, "250305T1200Z", "250305T1800Z")],
        ),
    )
    .await;

    sqlx::query(
        "INSERT INTO product_polygons (product_id, segment_index, polygon_index, polygon_wkt, polygon_geom)
         VALUES ($1, 0, 0, 'POLYGON((-96.0 41.4,-95.8 41.4,-95.8 41.6,-96.0 41.6,-96.0 41.4))',
                 ST_GeomFromText('POLYGON((-96.0 41.4,-95.8 41.4,-95.8 41.6,-96.0 41.6,-96.0 41.4))', 4326))",
    )
    .bind(polygon_id)
    .execute(&sink.pool())
    .await
    .expect("polygon insert should succeed");
    sqlx::query(
        "INSERT INTO product_time_mot_loc (product_id, segment_index, entry_index, time_utc, direction_degrees, speed_kt, path_wkt, path_geom)
         VALUES ($1, 0, 0, TIMESTAMPTZ '2025-03-05T13:00:00Z', 300, 25,
                 'LINESTRING(-95.9 41.5, -95.7 41.7)',
                 ST_GeomFromText('LINESTRING(-95.9 41.5, -95.7 41.7)', 4326))",
    )
    .bind(path_id)
    .execute(&sink.pool())
    .await
    .expect("path insert should succeed");

    let cells = sink
        .list_cell_aggregate(emwin_db::CellAggregateQuery {
            filters: emwin_db::ProductListQuery {
                office: Some("KOAX".to_string()),
                limit: 100,
                ..Default::default()
            },
            measure: emwin_db::CellMeasure::ProductCount,
            precision: 5,
            limit: 20,
        })
        .await
        .expect("cell aggregate should succeed");

    assert!(!cells.completeness.partial);
    assert!(
        cells.items.iter().map(|item| item.count).sum::<i64>() >= 2,
        "polygon-only and path-only products should contribute to cells"
    );

    cleanup_rows(&sink, &filenames, &[]).await;
}

#[tokio::test]
async fn archived_aggregates_apply_issue_and_vtec_row_filters() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let filenames = ["AGG-FILTERS-1.TXT"];
    cleanup_rows(&sink, &filenames, &[]).await;

    let product_id = persist_metadata(
        &sink,
        build_vtec_metadata(
            filenames[0],
            1_741_182_650,
            "NEC001-051300-",
            &[
                vtec_line('O', "NEW", 3701, "250305T1200Z", "250305T1800Z"),
                vtec_line('O', "CON", 3702, "250305T1200Z", "250305T1800Z"),
            ],
        ),
    )
    .await;

    sqlx::query(
        "INSERT INTO product_issues (product_id, kind, code, message, line)
         VALUES
            ($1, 'text_product_parse', 'invalid_wmo_header', 'failed', NULL),
            ($1, 'text_product_parse', 'missing_body', 'failed', NULL)",
    )
    .bind(product_id)
    .execute(&sink.pool())
    .await
    .expect("issue inserts should succeed");

    let issue_facets = sink
        .list_facet_aggregate(emwin_db::FacetAggregateQuery {
            filters: emwin_db::ProductListQuery {
                issue_code: Some("invalid_wmo_header".to_string()),
                limit: 100,
                ..Default::default()
            },
            dimension: emwin_db::FacetDimension::IssueCode,
            limit: 20,
        })
        .await
        .expect("issue facet query should succeed");
    assert_eq!(issue_facets.items.len(), 1);
    assert_eq!(issue_facets.items[0].value, "invalid_wmo_header");
    assert_eq!(issue_facets.items[0].count, 1);

    let issue_timeseries = sink
        .list_timeseries_aggregate(emwin_db::TimeseriesAggregateQuery {
            filters: emwin_db::ProductListQuery {
                issue_code: Some("invalid_wmo_header".to_string()),
                limit: 100,
                ..Default::default()
            },
            measure: emwin_db::TimeseriesMeasure::IssueCount,
            start: chrono::Utc
                .timestamp_opt(1_741_181_800, 0)
                .single()
                .expect("valid start"),
            end: chrono::Utc
                .timestamp_opt(1_741_185_400, 0)
                .single()
                .expect("valid end"),
            bucket: emwin_db::TimeseriesBucket::Hour,
        })
        .await
        .expect("issue timeseries should succeed");
    assert_eq!(
        issue_timeseries
            .items
            .iter()
            .map(|item| item.count)
            .sum::<i64>(),
        1
    );

    let incident_timeseries = sink
        .list_timeseries_aggregate(emwin_db::TimeseriesAggregateQuery {
            filters: emwin_db::ProductListQuery {
                vtec_action: Some("NEW".to_string()),
                limit: 100,
                ..Default::default()
            },
            measure: emwin_db::TimeseriesMeasure::IncidentCount,
            start: chrono::Utc
                .timestamp_opt(1_741_181_800, 0)
                .single()
                .expect("valid start"),
            end: chrono::Utc
                .timestamp_opt(1_741_185_400, 0)
                .single()
                .expect("valid end"),
            bucket: emwin_db::TimeseriesBucket::Hour,
        })
        .await
        .expect("incident timeseries should succeed");
    assert_eq!(
        incident_timeseries
            .items
            .iter()
            .map(|item| item.count)
            .sum::<i64>(),
        1
    );

    cleanup_rows(&sink, &filenames, &[]).await;
}

#[tokio::test]
async fn archived_queries_reject_invalid_cursor_and_spatial_inputs() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let invalid_cursor = sink
        .list_archived_products(emwin_db::ProductListQuery {
            cursor: Some("%%%".to_string()),
            ..Default::default()
        })
        .await
        .expect_err("invalid cursor should fail");
    assert!(invalid_cursor.to_string().contains("invalid cursor"));

    let invalid_feature_cursor = sink
        .list_archived_features(emwin_db::FeatureListQuery {
            filters: emwin_db::ProductListQuery {
                cursor: Some("%%%".to_string()),
                ..Default::default()
            },
            kind: None,
        })
        .await
        .expect_err("invalid feature cursor should fail");
    assert!(
        invalid_feature_cursor
            .to_string()
            .contains("invalid cursor")
    );

    let partial_bbox = sink
        .list_archived_products(emwin_db::ProductListQuery {
            min_lat: Some(41.0),
            max_lat: Some(42.0),
            min_lon: Some(-97.0),
            ..Default::default()
        })
        .await
        .expect_err("partial bbox should fail");
    assert!(
        partial_bbox
            .to_string()
            .contains("must be provided together")
    );

    let invalid_distance = sink
        .list_archived_features(emwin_db::FeatureListQuery {
            filters: emwin_db::ProductListQuery {
                lat: Some(41.0),
                lon: Some(-96.0),
                distance_miles: Some(0.0),
                ..Default::default()
            },
            kind: None,
        })
        .await
        .expect_err("non-positive distance should fail");
    assert!(
        invalid_distance
            .to_string()
            .contains("distance_miles must be a finite value greater than 0")
    );

    let missing_lon = sink
        .list_cell_aggregate(emwin_db::CellAggregateQuery {
            filters: emwin_db::ProductListQuery {
                lat: Some(41.0),
                ..Default::default()
            },
            measure: emwin_db::CellMeasure::ProductCount,
            precision: 5,
            limit: 10,
        })
        .await
        .expect_err("missing lon should fail");
    assert!(
        missing_lon
            .to_string()
            .contains("lat and lon must be provided together")
    );
}

#[tokio::test]
async fn archived_timeseries_rejects_oversized_bucket_count() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let error = sink
        .list_timeseries_aggregate(emwin_db::TimeseriesAggregateQuery {
            filters: emwin_db::ProductListQuery::default(),
            measure: emwin_db::TimeseriesMeasure::ProductCount,
            start: chrono::Utc
                .timestamp_opt(1_741_181_800, 0)
                .single()
                .expect("valid start"),
            end: chrono::Utc
                .timestamp_opt(1_744_872_600, 0)
                .single()
                .expect("valid end"),
            bucket: emwin_db::TimeseriesBucket::Hour,
        })
        .await
        .expect_err("oversized timeseries should fail");
    assert!(error.to_string().contains("more than 1000 buckets"));
}
