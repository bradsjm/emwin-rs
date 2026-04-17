use crate::config::{LiveConfigRequest, build_qbt_receiver_config, build_wxwire_receiver_config};
use crate::error::{LiveError, LiveResult};
use crate::ingest::{run_incident_event_relay_loop, run_qbt_ingest_loop, run_wxwire_ingest_loop};
use crate::persistence::{
    FilePersistenceRuntime, run_incident_cleanup_loop, shutdown_runtime,
    start_runtime_with_postgres,
};
use crate::shared::lock_unpoisoned;
use crate::types::{
    AppState, IncidentBroadcastEvent, LiveBroadcastEvent, LiveOptions, LiveStatsSnapshot,
    LiveTelemetry, ReceiverKind,
};
use emwin_service::{
    ArchiveQueryService, ArchivedFeature, ArchivedIssue, ArchivedIssueListQuery, ArchivedPayload,
    ArchivedProductDetail, ArchivedProductSummary, CellAggregateQuery, CellAggregateResult,
    FacetAggregateQuery, FacetAggregateResult, FeatureListQuery, IncidentDetail, IncidentKey,
    IncidentListQuery, IncidentProductsQuery, IncidentSummary, PaginatedResponse, PersistenceStats,
    ProductListQuery, RetainedFile, ServiceError, ServiceResult, SourceKind,
    TimeseriesAggregateQuery, TimeseriesAggregateResult,
};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::{Mutex, watch};
use tokio::task::JoinHandle;

enum RuntimeMode {
    IngestOnly,
    PersistenceOnly {
        persistence_runtime: FilePersistenceRuntime,
    },
    PersistenceWithArchive {
        cleanup: JoinHandle<LiveResult<()>>,
        incident_relay: JoinHandle<LiveResult<()>>,
        persistence_runtime: FilePersistenceRuntime,
    },
}

struct RuntimeTasks {
    ingest: JoinHandle<LiveResult<()>>,
    mode: RuntimeMode,
}

struct LiveRuntimeInner {
    state: Arc<AppState>,
    shutdown_tx: watch::Sender<bool>,
    tasks: Mutex<Option<RuntimeTasks>>,
}

#[derive(Clone)]
/// Public runtime handle for live ingest and archive serving.
pub struct LiveRuntime {
    inner: Arc<LiveRuntimeInner>,
}

impl LiveRuntime {
    /// Starts a live ingest runtime from normalized options.
    pub async fn start(options: LiveOptions) -> LiveResult<Self> {
        let LiveOptions {
            receiver,
            username,
            password,
            raw_servers,
            server_list_path,
            output_dir,
            post_process_archives,
            quiet,
            persistence_queue_capacity,
            postgres_database_url,
            max_db_connections,
            file_retention_secs,
            max_retained_files,
        } = options;

        if postgres_database_url.is_some() && output_dir.is_none() {
            return Err(LiveError::invalid_argument(
                "--persist-database-url requires --output-dir for blob storage",
            ));
        }

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let started_persistence = match output_dir {
            Some(path) => Some(
                start_runtime_with_postgres(
                    path,
                    persistence_queue_capacity,
                    postgres_database_url.as_deref(),
                    max_db_connections,
                    "emwin-live",
                )
                .await?,
            ),
            None => None,
        };
        let cleanup_sink = started_persistence
            .as_ref()
            .and_then(|started| started.postgres_sink.clone());
        let archive_sink = started_persistence
            .as_ref()
            .and_then(|started| started.postgres_sink.clone());
        let persistence_runtime = started_persistence.map(|started| started.runtime);
        let persistence_producer = persistence_runtime
            .as_ref()
            .map(|runtime| runtime.producer());

        let state = AppState::new(
            persistence_producer.clone(),
            archive_sink.clone(),
            quiet,
            max_retained_files,
            file_retention_secs,
        );

        if let Some(postgres_sink) = archive_sink.as_ref() {
            match postgres_sink
                .expire_active_incidents(chrono::Utc::now())
                .await
            {
                Ok(result) if result.expired_count > 0 => {
                    tracing::info!(
                        backend = "database",
                        target = %postgres_sink.describe_target(),
                        expired_count = result.expired_count,
                        "expired stale incidents during startup"
                    );
                }
                Ok(_) => {}
                Err(err) => {
                    {
                        let mut guard = lock_unpoisoned(&state.archive_last_error);
                        *guard = Some(err.to_string());
                    }
                    tracing::warn!(
                        backend = "database",
                        target = %postgres_sink.describe_target(),
                        stage = "incident_cleanup",
                        error = %err,
                        "startup incident cleanup skipped; will retry in background"
                    );
                }
            }
        }

        let ingest = match receiver {
            ReceiverKind::Qbt => {
                let config = build_qbt_receiver_config(LiveConfigRequest {
                    username: Some(username),
                    password,
                    raw_servers,
                    server_list_path,
                    idle_timeout_secs: 90,
                    qbt_watchdog_timeout_secs: 20,
                    username_context: "server mode",
                    password_context: "server mode",
                })?;
                tokio::spawn(run_qbt_ingest_loop(
                    config,
                    Arc::clone(&state),
                    post_process_archives,
                    persistence_producer.clone(),
                    shutdown_rx.clone(),
                ))
            }
            ReceiverKind::Wxwire => {
                let config = build_wxwire_receiver_config(LiveConfigRequest {
                    username: Some(username),
                    password,
                    raw_servers,
                    server_list_path,
                    idle_timeout_secs: 90,
                    qbt_watchdog_timeout_secs: 0,
                    username_context: "wxwire server mode",
                    password_context: "wxwire server mode",
                })?;
                tokio::spawn(run_wxwire_ingest_loop(
                    config,
                    Arc::clone(&state),
                    post_process_archives,
                    persistence_producer.clone(),
                    shutdown_rx.clone(),
                ))
            }
        };
        let mode = match (cleanup_sink, archive_sink, persistence_runtime) {
            (Some(cleanup_sink), Some(archive_sink), Some(persistence_runtime)) => {
                RuntimeMode::PersistenceWithArchive {
                    cleanup: tokio::spawn(run_incident_cleanup_loop(
                        cleanup_sink,
                        shutdown_rx.clone(),
                    )),
                    incident_relay: tokio::spawn(run_incident_event_relay_loop(
                        archive_sink,
                        Arc::clone(&state),
                        shutdown_rx.clone(),
                    )),
                    persistence_runtime,
                }
            }
            (None, None, Some(persistence_runtime)) => RuntimeMode::PersistenceOnly {
                persistence_runtime,
            },
            (None, None, None) => RuntimeMode::IngestOnly,
            _ => {
                return Err(LiveError::runtime(
                    "invalid runtime state: archive services require persistence runtime"
                        .to_string(),
                ));
            }
        };

        Ok(Self {
            inner: Arc::new(LiveRuntimeInner {
                state,
                shutdown_tx,
                tasks: Mutex::new(Some(RuntimeTasks { ingest, mode })),
            }),
        })
    }

    /// Subscribes to live broadcast events.
    pub fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<LiveBroadcastEvent> {
        self.inner.state.event_tx.subscribe()
    }

    /// Subscribes to incident change broadcasts when archive support is enabled.
    pub fn subscribe_incident_changes(
        &self,
    ) -> Option<tokio::sync::broadcast::Receiver<IncidentBroadcastEvent>> {
        self.inner
            .state
            .archive
            .as_ref()
            .map(|_| self.inner.state.incident_event_tx.subscribe())
    }

    /// Returns the current receiver telemetry snapshot.
    pub fn telemetry_snapshot(&self) -> LiveTelemetry {
        lock_unpoisoned(&self.inner.state.telemetry).clone()
    }

    /// Returns a live runtime snapshot for `/metrics` and diagnostics.
    pub fn stats_snapshot(&self) -> LiveStatsSnapshot {
        let state = &self.inner.state;
        let upstream_endpoint = lock_unpoisoned(&state.upstream_endpoint).clone();

        LiveStatsSnapshot {
            uptime_secs: state.started_at.elapsed().as_secs(),
            data_blocks_total: state.data_blocks_total.load(Ordering::Relaxed),
            active_servers: state.active_servers.load(Ordering::Relaxed),
            retained_files: lock_unpoisoned(&state.retained_files).len(),
            upstream_endpoint,
            persistence: state
                .persistence
                .as_ref()
                .map(crate::persistence::FilePersistenceProducer::stats_snapshot)
                .map(|stats| PersistenceStats {
                    queue_len: stats.queue_len,
                    queue_capacity: stats.queue_capacity,
                    enqueued_total: stats.enqueued_total,
                    evicted_total: stats.evicted_total,
                    persisted_total: stats.persisted_total,
                    failed_total: stats.failed_total,
                }),
        }
    }

    /// Returns the last archive error observed by the runtime.
    pub fn archive_last_error(&self) -> Option<String> {
        lock_unpoisoned(&self.inner.state.archive_last_error).clone()
    }

    /// Returns whether archive persistence is configured.
    pub fn archive_configured(&self) -> bool {
        self.inner.state.archive.is_some()
    }

    /// Returns the configured archive sink when one is available.
    pub fn alert_store(&self) -> Option<emwin_db::PostgresMetadataSink> {
        self.inner.state.archive.clone()
    }

    /// Returns the total number of archive errors observed.
    pub fn archive_errors_total(&self) -> u64 {
        self.inner
            .state
            .archive_errors_total
            .load(Ordering::Relaxed)
    }

    /// Returns the total number of archive pool timeout errors observed.
    pub fn archive_pool_timeouts_total(&self) -> u64 {
        self.inner
            .state
            .archive_pool_timeouts_total
            .load(Ordering::Relaxed)
    }

    /// Returns the currently retained completed files.
    pub fn list_retained_files(&self) -> Vec<emwin_service::CompletedFileMetadata> {
        lock_unpoisoned(&self.inner.state.retained_files).list()
    }

    /// Looks up one retained file by filename.
    pub fn get_retained_file(&self, filename: &str) -> Option<RetainedFile> {
        lock_unpoisoned(&self.inner.state.retained_files).get(filename)
    }

    /// Signals shutdown and waits for background tasks to stop.
    pub async fn shutdown(&self) -> LiveResult<()> {
        let mut guard = self.inner.tasks.lock().await;
        let Some(tasks) = guard.take() else {
            return Ok(());
        };

        let _ = self.inner.shutdown_tx.send(true);

        await_task(tasks.ingest, "ingest").await?;
        match tasks.mode {
            RuntimeMode::IngestOnly => {}
            RuntimeMode::PersistenceOnly {
                persistence_runtime,
            } => {
                let _ = shutdown_runtime(persistence_runtime).await?;
            }
            RuntimeMode::PersistenceWithArchive {
                cleanup,
                incident_relay,
                persistence_runtime,
            } => {
                await_task(cleanup, "cleanup").await?;
                await_task(incident_relay, "incident relay").await?;
                let _ = shutdown_runtime(persistence_runtime).await?;
            }
        }

        Ok(())
    }

    pub(crate) fn from_test_state(
        retained_files: Vec<(String, Vec<u8>, u64, SourceKind)>,
        telemetry: crate::types::LiveTelemetry,
        archive: Option<emwin_db::PostgresMetadataSink>,
        persistence: Option<emwin_db::PersistenceProducer<emwin_db::CompletedFileMetadata>>,
        upstream_endpoint: Option<String>,
        active_servers: usize,
        archive_status: Option<(String, u64, u64)>,
    ) -> Self {
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);
        let state = AppState::new(persistence, archive, true, 32, 60);
        {
            let mut retained = lock_unpoisoned(&state.retained_files);
            for (filename, data, timestamp_utc, origin) in retained_files {
                let origin = match origin {
                    SourceKind::Qbt => emwin_protocol::ingest::ProductOrigin::Qbt,
                    SourceKind::WxWire {
                        message_id,
                        subject,
                        delay_stamp_utc,
                    } => emwin_protocol::ingest::ProductOrigin::WxWire {
                        message_id,
                        subject,
                        delay_stamp_utc,
                    },
                    SourceKind::Unknown => {
                        panic!("LiveRuntime::new_for_tests requires a concrete source kind")
                    }
                };
                retained.insert(
                    filename,
                    data.into(),
                    timestamp_utc,
                    origin,
                    std::time::SystemTime::now(),
                );
            }
        }
        {
            let mut guard = lock_unpoisoned(&state.telemetry);
            *guard = telemetry;
        }
        {
            let mut guard = lock_unpoisoned(&state.upstream_endpoint);
            *guard = upstream_endpoint;
        }
        let (archive_last_error, archive_errors_total, archive_pool_timeouts_total) =
            match archive_status {
                Some((last_error, errors_total, pool_timeouts_total)) => {
                    (Some(last_error), errors_total, pool_timeouts_total)
                }
                None => (None, 0, 0),
            };
        {
            let mut guard = lock_unpoisoned(&state.archive_last_error);
            *guard = archive_last_error;
        }
        state
            .archive_errors_total
            .store(archive_errors_total, Ordering::Relaxed);
        state
            .archive_pool_timeouts_total
            .store(archive_pool_timeouts_total, Ordering::Relaxed);
        state
            .active_servers
            .store(active_servers, Ordering::Relaxed);

        Self {
            inner: Arc::new(LiveRuntimeInner {
                state,
                shutdown_tx,
                tasks: Mutex::new(None),
            }),
        }
    }

    /// Returns the archive query service when archive persistence is enabled.
    pub fn archive_query_service(&self) -> Option<Arc<dyn ArchiveQueryService>> {
        let archive = self.inner.state.archive.clone()?;
        Some(Arc::new(TrackedArchiveQueryService {
            state: Arc::clone(&self.inner.state),
            archive,
        }))
    }
}

fn map_service_error(err: emwin_db::PersistError) -> ServiceError {
    match err {
        emwin_db::PersistError::InvalidRequest(message) => ServiceError::InvalidRequest(message),
        emwin_db::PersistError::InvalidConfig(message) => ServiceError::InvalidConfig(message),
        emwin_db::PersistError::Io(io) => ServiceError::Io(io),
        other => ServiceError::Runtime(other.to_string()),
    }
}

fn record_archive_result<T>(
    state: &Arc<AppState>,
    result: Result<T, emwin_db::PersistError>,
) -> ServiceResult<T> {
    match result {
        Ok(value) => {
            let mut guard = lock_unpoisoned(&state.archive_last_error);
            *guard = None;
            Ok(value)
        }
        Err(err) => {
            let message = err.to_string();
            state.archive_errors_total.fetch_add(1, Ordering::Relaxed);
            if message.contains("pool timed out while waiting for an open connection") {
                state
                    .archive_pool_timeouts_total
                    .fetch_add(1, Ordering::Relaxed);
            }
            let mut guard = lock_unpoisoned(&state.archive_last_error);
            *guard = Some(message);
            Err(map_service_error(err))
        }
    }
}

#[derive(Clone)]
struct TrackedArchiveQueryService {
    state: Arc<AppState>,
    archive: emwin_db::PostgresMetadataSink,
}

impl ArchiveQueryService for TrackedArchiveQueryService {
    fn list_incidents(
        &self,
        query: IncidentListQuery,
    ) -> emwin_service::archive::BoxFuture<'_, ServiceResult<PaginatedResponse<IncidentSummary>>>
    {
        Box::pin(async move {
            record_archive_result(&self.state, self.archive.list_incidents(query).await)
        })
    }

    fn get_incident<'a>(
        &'a self,
        key: &'a IncidentKey,
    ) -> emwin_service::archive::BoxFuture<'a, ServiceResult<Option<IncidentDetail>>> {
        Box::pin(
            async move { record_archive_result(&self.state, self.archive.get_incident(key).await) },
        )
    }

    fn list_incident_products<'a>(
        &'a self,
        key: &'a IncidentKey,
        query: IncidentProductsQuery,
    ) -> emwin_service::archive::BoxFuture<
        'a,
        ServiceResult<PaginatedResponse<ArchivedProductSummary>>,
    > {
        Box::pin(async move {
            record_archive_result(
                &self.state,
                self.archive.list_incident_products(key, query).await,
            )
        })
    }

    fn list_archived_products(
        &self,
        query: ProductListQuery,
    ) -> emwin_service::archive::BoxFuture<
        '_,
        ServiceResult<PaginatedResponse<ArchivedProductSummary>>,
    > {
        Box::pin(async move {
            record_archive_result(
                &self.state,
                self.archive.list_archived_products(query).await,
            )
        })
    }

    fn get_archived_product(
        &self,
        product_id: i64,
    ) -> emwin_service::archive::BoxFuture<'_, ServiceResult<Option<ArchivedProductDetail>>> {
        Box::pin(async move {
            record_archive_result(
                &self.state,
                self.archive.get_archived_product(product_id).await,
            )
        })
    }

    fn list_archived_issues(
        &self,
        query: ArchivedIssueListQuery,
    ) -> emwin_service::archive::BoxFuture<'_, ServiceResult<PaginatedResponse<ArchivedIssue>>>
    {
        Box::pin(async move {
            record_archive_result(&self.state, self.archive.list_archived_issues(query).await)
        })
    }

    fn get_archived_issue(
        &self,
        issue_id: i64,
    ) -> emwin_service::archive::BoxFuture<'_, ServiceResult<Option<ArchivedIssue>>> {
        Box::pin(async move {
            record_archive_result(&self.state, self.archive.get_archived_issue(issue_id).await)
        })
    }

    fn read_archived_payload(
        &self,
        product_id: i64,
    ) -> emwin_service::archive::BoxFuture<'_, ServiceResult<Option<ArchivedPayload>>> {
        Box::pin(async move {
            record_archive_result(
                &self.state,
                self.archive.read_archived_payload(product_id).await,
            )
        })
    }

    fn list_archived_features(
        &self,
        query: FeatureListQuery,
    ) -> emwin_service::archive::BoxFuture<'_, ServiceResult<PaginatedResponse<ArchivedFeature>>>
    {
        Box::pin(async move {
            record_archive_result(
                &self.state,
                self.archive.list_archived_features(query).await,
            )
        })
    }

    fn list_facet_aggregate(
        &self,
        query: FacetAggregateQuery,
    ) -> emwin_service::archive::BoxFuture<'_, ServiceResult<FacetAggregateResult>> {
        Box::pin(async move {
            record_archive_result(&self.state, self.archive.list_facet_aggregate(query).await)
        })
    }

    fn list_timeseries_aggregate(
        &self,
        query: TimeseriesAggregateQuery,
    ) -> emwin_service::archive::BoxFuture<'_, ServiceResult<TimeseriesAggregateResult>> {
        Box::pin(async move {
            record_archive_result(
                &self.state,
                self.archive.list_timeseries_aggregate(query).await,
            )
        })
    }

    fn list_cell_aggregate(
        &self,
        query: CellAggregateQuery,
    ) -> emwin_service::archive::BoxFuture<'_, ServiceResult<CellAggregateResult>> {
        Box::pin(async move {
            record_archive_result(&self.state, self.archive.list_cell_aggregate(query).await)
        })
    }
}

async fn await_task(task: JoinHandle<LiveResult<()>>, name: &str) -> LiveResult<()> {
    match task.await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => Err(LiveError::runtime(format!("{name} task failed: {err}"))),
        Err(err) => Err(LiveError::runtime(format!(
            "{name} task join failed: {err}"
        ))),
    }
}
