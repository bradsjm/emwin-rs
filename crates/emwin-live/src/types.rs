#![allow(missing_docs)]

use crate::persistence::FilePersistenceProducer;
use crate::product_processor::ProductProcessorProducer;
use crate::retained::RetainedFiles;
use emwin_db::PostgresMetadataSink;
pub(crate) use emwin_service::{
    IncidentBroadcastEvent, LiveBroadcastEvent, LiveEventKind, LiveStatsSnapshot, LiveTelemetry,
};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverKind {
    Qbt,
    Wxwire,
}

#[derive(Debug, Clone)]
pub struct LiveOptions {
    pub receiver: ReceiverKind,
    pub username: String,
    pub password: Option<String>,
    pub raw_servers: Vec<String>,
    pub server_list_path: Option<String>,
    pub output_dir: Option<String>,
    pub post_process_archives: bool,
    pub quiet: bool,
    pub persistence_queue_capacity: usize,
    pub postgres_database_url: Option<String>,
    pub max_db_connections: u32,
    pub file_retention_secs: u64,
    pub max_retained_files: usize,
}

#[derive(Debug)]
pub(crate) struct AppState {
    pub(crate) event_tx: broadcast::Sender<LiveBroadcastEvent>,
    pub(crate) incident_event_tx: broadcast::Sender<IncidentBroadcastEvent>,
    pub(crate) retained_files: Mutex<RetainedFiles>,
    pub(crate) telemetry: Mutex<LiveTelemetry>,
    pub(crate) product_processor: ProductProcessorProducer,
    pub(crate) persistence: Option<FilePersistenceProducer>,
    pub(crate) archive: Option<PostgresMetadataSink>,
    pub(crate) next_event_id: AtomicU64,
    pub(crate) next_incident_event_id: AtomicU64,
    pub(crate) data_blocks_total: AtomicU64,
    pub(crate) archive_errors_total: AtomicU64,
    pub(crate) archive_pool_timeouts_total: AtomicU64,
    pub(crate) active_servers: AtomicUsize,
    pub(crate) started_at: Instant,
    pub(crate) upstream_endpoint: Mutex<Option<String>>,
    pub(crate) archive_last_error: Mutex<Option<String>>,
    pub(crate) quiet: bool,
}

impl AppState {
    pub(crate) fn new(
        persistence: Option<FilePersistenceProducer>,
        product_processor: ProductProcessorProducer,
        archive: Option<PostgresMetadataSink>,
        quiet: bool,
        max_retained_files: usize,
        file_retention_secs: u64,
    ) -> Arc<Self> {
        Arc::new(Self {
            event_tx: broadcast::channel(4096).0,
            incident_event_tx: broadcast::channel(4096).0,
            retained_files: Mutex::new(RetainedFiles::new(
                max_retained_files.max(1),
                Duration::from_secs(file_retention_secs.max(1)),
            )),
            telemetry: Mutex::new(LiveTelemetry::Unavailable),
            product_processor,
            persistence,
            archive,
            next_event_id: AtomicU64::new(1),
            next_incident_event_id: AtomicU64::new(1),
            data_blocks_total: AtomicU64::new(0),
            archive_errors_total: AtomicU64::new(0),
            archive_pool_timeouts_total: AtomicU64::new(0),
            active_servers: AtomicUsize::new(0),
            started_at: Instant::now(),
            upstream_endpoint: Mutex::new(None),
            archive_last_error: Mutex::new(None),
            quiet,
        })
    }
}
