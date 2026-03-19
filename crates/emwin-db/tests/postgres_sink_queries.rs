mod common;

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
