use crate::live::server::types::{
    AppState, ArchiveIssuePayload, EventsQuery, IncidentEventPayload, IncidentSummaryPayload,
    TelemetryPayload,
};
use crate::live::server_support::RetainedFiles;
use emwin_db::{ArchivedIssue, IncidentChangeAction, IncidentChangeTrigger, IncidentSummary};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, watch};

mod auth;
mod docs;
mod events_and_incidents;
mod files;
mod metrics;

fn test_state(max_clients: usize) -> Arc<AppState> {
    let (_, shutdown_rx) = watch::channel(false);
    Arc::new(AppState {
        event_tx: broadcast::channel(32).0,
        incident_event_tx: broadcast::channel(32).0,
        shutdown_rx,
        retained_files: std::sync::Mutex::new(RetainedFiles::new(32, Duration::from_secs(60))),
        telemetry: std::sync::Mutex::new(TelemetryPayload::Unavailable),
        persistence: None,
        archive: None,
        connected_clients: AtomicUsize::new(0),
        max_clients,
        next_event_id: AtomicU64::new(1),
        next_incident_event_id: AtomicU64::new(1),
        data_blocks_total: AtomicU64::new(0),
        received_servers: AtomicUsize::new(0),
        received_sat_servers: AtomicUsize::new(0),
        started_at: Instant::now(),
        upstream_endpoint: std::sync::Mutex::new(None),
        openapi_auth_token: None,
        quiet: true,
    })
}

fn test_state_with_auth(max_clients: usize, token: &str) -> Arc<AppState> {
    let (_, shutdown_rx) = watch::channel(false);
    Arc::new(AppState {
        event_tx: broadcast::channel(32).0,
        incident_event_tx: broadcast::channel(32).0,
        shutdown_rx,
        retained_files: std::sync::Mutex::new(RetainedFiles::new(32, Duration::from_secs(60))),
        telemetry: std::sync::Mutex::new(TelemetryPayload::Unavailable),
        persistence: None,
        archive: None,
        connected_clients: AtomicUsize::new(0),
        max_clients,
        next_event_id: AtomicU64::new(1),
        next_incident_event_id: AtomicU64::new(1),
        data_blocks_total: AtomicU64::new(0),
        received_servers: AtomicUsize::new(0),
        received_sat_servers: AtomicUsize::new(0),
        started_at: Instant::now(),
        upstream_endpoint: std::sync::Mutex::new(None),
        openapi_auth_token: Some(token.to_string()),
        quiet: true,
    })
}

fn test_state_with_archive(max_clients: usize) -> Arc<AppState> {
    let (_, shutdown_rx) = watch::channel(false);
    Arc::new(AppState {
        event_tx: broadcast::channel(32).0,
        incident_event_tx: broadcast::channel(32).0,
        shutdown_rx,
        retained_files: std::sync::Mutex::new(RetainedFiles::new(32, Duration::from_secs(60))),
        telemetry: std::sync::Mutex::new(TelemetryPayload::Unavailable),
        persistence: None,
        archive: Some(emwin_db::PostgresMetadataSink::new(
            emwin_db::PostgresConfig::new("postgres://example.invalid/emwin"),
        )),
        connected_clients: AtomicUsize::new(0),
        max_clients,
        next_event_id: AtomicU64::new(1),
        next_incident_event_id: AtomicU64::new(1),
        data_blocks_total: AtomicU64::new(0),
        received_servers: AtomicUsize::new(0),
        received_sat_servers: AtomicUsize::new(0),
        started_at: Instant::now(),
        upstream_endpoint: std::sync::Mutex::new(None),
        openapi_auth_token: None,
        quiet: true,
    })
}

fn empty_events_query() -> EventsQuery {
    EventsQuery {
        event: None,
        filename: None,
        source: None,
        pil: None,
        family: None,
        container: None,
        wmo_prefix: None,
        office: None,
        office_city: None,
        office_state: None,
        bbb_kind: None,
        cccc: None,
        ttaaii: None,
        afos: None,
        bbb: None,
        has_issues: None,
        issue_kind: None,
        issue_code: None,
        has_vtec: None,
        has_ugc: None,
        has_hvtec: None,
        has_latlon: None,
        has_time_mot_loc: None,
        has_wind_hail: None,
        state: None,
        county: None,
        zone: None,
        fire_zone: None,
        marine_zone: None,
        vtec_phenomena: None,
        vtec_significance: None,
        vtec_action: None,
        vtec_office: None,
        etn: None,
        hvtec_nwslid: None,
        hvtec_severity: None,
        hvtec_cause: None,
        hvtec_record: None,
        wind_hail_kind: None,
        lat: None,
        lon: None,
        distance_miles: None,
        min_wind_mph: None,
        min_hail_inches: None,
        min_size: None,
        max_size: None,
    }
}

fn incident_event_payload() -> IncidentEventPayload {
    IncidentEventPayload {
        action: IncidentChangeAction::Created,
        trigger: IncidentChangeTrigger::Persist,
        incident: IncidentSummaryPayload::from_incident(IncidentSummary {
            office: "KOAX".to_string(),
            phenomena: "FF".to_string(),
            significance: "W".to_string(),
            etn: 2001,
            current_status: "active".to_string(),
            latest_vtec_action: "NEW".to_string(),
            issued_at: chrono::Utc::now(),
            start_utc: None,
            end_utc: None,
            last_updated_at: chrono::Utc::now(),
            first_product_id: 10,
            latest_product_id: 10,
            latest_product_timestamp_utc: chrono::Utc::now(),
        }),
    }
}

fn archive_issue_payload() -> ArchiveIssuePayload {
    ArchiveIssuePayload::from_issue(ArchivedIssue {
        id: 7,
        product_id: 42,
        kind: "text_product_parse".to_string(),
        code: "invalid_wmo_header".to_string(),
        message: "failed to parse WMO header".to_string(),
        line: Some("INVALID HEADER".to_string()),
    })
}
