mod common;

use chrono::{TimeZone, Utc};
use common::*;
use emwin_db::{IncidentChangeAction, IncidentChangeTrigger};
use tokio::time::{Duration, timeout};

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
