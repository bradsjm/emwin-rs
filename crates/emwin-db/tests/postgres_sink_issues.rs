mod common;

use common::*;

#[tokio::test]
async fn archived_issue_queries_return_list_and_detail() {
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
    let issue_id = product_issue_id(&sink, product_id).await;

    let issues = sink
        .list_archived_issues(emwin_db::ArchivedIssueListQuery {
            product_id: Some(product_id),
            ..Default::default()
        })
        .await
        .expect("issue list query should succeed");
    assert_eq!(issues.items.len(), 1);
    assert_eq!(issues.items[0].id, issue_id);
    assert_eq!(issues.items[0].product_id, product_id);
    assert_eq!(issues.items[0].code, "invalid_wmo_header");

    let issue = sink
        .get_archived_issue(issue_id)
        .await
        .expect("issue detail query should succeed")
        .expect("issue should exist");
    assert_eq!(issue.id, issue_id);
    assert_eq!(issue.kind, "text_product_parse");
    assert_eq!(issue.code, "invalid_wmo_header");

    cleanup_rows(&sink, &[&metadata.filename], &[incident_key]).await;
}

#[tokio::test]
async fn archived_issue_queries_support_filters_and_cursor_pagination() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let filenames = ["ISSUE-PAGE-1.TXT", "ISSUE-PAGE-2.TXT", "ISSUE-PAGE-3.TXT"];
    let incident_keys = [
        TestIncidentKey {
            office: "KOAX",
            phenomena: "FF",
            significance: "W",
            etn: 4101,
        },
        TestIncidentKey {
            office: "KOAX",
            phenomena: "FF",
            significance: "W",
            etn: 4102,
        },
        TestIncidentKey {
            office: "KOAX",
            phenomena: "FF",
            significance: "W",
            etn: 4103,
        },
    ];
    cleanup_rows(&sink, &filenames, &incident_keys).await;

    for (index, filename) in filenames.iter().enumerate() {
        let metadata = emwin_db::CompletedFileMetadata::build(
            filename,
            1_741_180_000 + u64::try_from(index).expect("index should fit") * 60,
            emwin_protocol::ingest::ProductOrigin::Qbt,
            b"000 \nINVALID HEADER\nAFDBOX\nBody\n",
        );
        persist_metadata(&sink, metadata).await;
    }

    let first_page = sink
        .list_archived_issues(emwin_db::ArchivedIssueListQuery {
            kind: Some("text_product_parse".to_string()),
            code: Some("invalid_wmo_header".to_string()),
            limit: 2,
            ..Default::default()
        })
        .await
        .expect("first issue page should succeed");
    assert_eq!(first_page.items.len(), 2);
    assert!(first_page.next_cursor.is_some());

    let second_page = sink
        .list_archived_issues(emwin_db::ArchivedIssueListQuery {
            kind: Some("text_product_parse".to_string()),
            code: Some("invalid_wmo_header".to_string()),
            limit: 2,
            cursor: first_page.next_cursor,
            ..Default::default()
        })
        .await
        .expect("second issue page should succeed");
    assert_eq!(second_page.items.len(), 1);
    assert_eq!(second_page.next_cursor, None);

    cleanup_rows(&sink, &filenames, &incident_keys).await;
}
