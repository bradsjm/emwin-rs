use crate::config::{LiveConfigRequest, LiveReceiverConfig, build_live_receiver_config};
use crate::error::{LiveError, LiveResult};
use crate::ingest::{run_incident_event_relay_loop, run_qbt_ingest_loop, run_wxwire_ingest_loop};
use crate::persistence::{
    FilePersistenceRuntime, run_incident_cleanup_loop, shutdown_runtime,
    start_runtime_with_postgres,
};
use crate::types::{
    AppState, IncidentBroadcastEvent, LiveBroadcastEvent, LiveOptions, LiveStatsSnapshot,
    LiveTelemetry, ReceiverKind,
};
use emwin_service::{
    ArchiveQueryService, ArchivedFeature, ArchivedIssue, ArchivedIssueListQuery, ArchivedPayload,
    ArchivedProductDetail, ArchivedProductSummary, CellAggregateQuery, CellAggregateResult,
    FacetAggregateQuery, FacetAggregateResult, FeatureListQuery, IncidentChangeStream,
    IncidentDetail, IncidentKey, IncidentListQuery, IncidentProductsQuery, IncidentSummary,
    LiveEventService, PaginatedResponse, PersistenceStats, ProductListQuery, RetainedFile,
    RetainedFileService, ServiceError, ServiceResult, SourceKind, TimeseriesAggregateQuery,
    TimeseriesAggregateResult,
};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::sync::{Mutex, watch};
use tokio::task::JoinHandle;

struct RuntimeTasks {
    ingest: JoinHandle<LiveResult<()>>,
    cleanup: Option<JoinHandle<LiveResult<()>>>,
    incident_relay: Option<JoinHandle<LiveResult<()>>>,
    persistence_runtime: Option<FilePersistenceRuntime>,
}

struct LiveRuntimeInner {
    state: Arc<AppState>,
    shutdown_tx: watch::Sender<bool>,
    tasks: Mutex<Option<RuntimeTasks>>,
}

#[derive(Clone)]
pub struct LiveRuntime {
    inner: Arc<LiveRuntimeInner>,
}

impl LiveRuntime {
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
                let LiveReceiverConfig::Qbt(config) =
                    build_live_receiver_config(LiveConfigRequest {
                        receiver: ReceiverKind::Qbt,
                        username: Some(username),
                        password,
                        raw_servers,
                        server_list_path,
                        idle_timeout_secs: 90,
                        qbt_watchdog_timeout_secs: 20,
                        username_context: "server mode",
                        password_context: "server mode",
                    })?
                else {
                    unreachable!("qbt live mode must build qbt config");
                };
                tokio::spawn(run_qbt_ingest_loop(
                    config,
                    Arc::clone(&state),
                    post_process_archives,
                    persistence_producer.clone(),
                    shutdown_rx.clone(),
                ))
            }
            ReceiverKind::Wxwire => {
                let LiveReceiverConfig::WxWire(config) =
                    build_live_receiver_config(LiveConfigRequest {
                        receiver: ReceiverKind::Wxwire,
                        username: Some(username),
                        password,
                        raw_servers,
                        server_list_path,
                        idle_timeout_secs: 90,
                        qbt_watchdog_timeout_secs: 0,
                        username_context: "wxwire server mode",
                        password_context: "wxwire server mode",
                    })?
                else {
                    unreachable!("wxwire live mode must build wxwire config");
                };
                tokio::spawn(run_wxwire_ingest_loop(
                    config,
                    Arc::clone(&state),
                    post_process_archives,
                    persistence_producer.clone(),
                    shutdown_rx.clone(),
                ))
            }
        };
        let cleanup = cleanup_sink
            .map(|sink| tokio::spawn(run_incident_cleanup_loop(sink, shutdown_rx.clone())));
        let incident_relay = archive_sink.map(|sink| {
            tokio::spawn(run_incident_event_relay_loop(
                sink,
                Arc::clone(&state),
                shutdown_rx.clone(),
            ))
        });

        Ok(Self {
            inner: Arc::new(LiveRuntimeInner {
                state,
                shutdown_tx,
                tasks: Mutex::new(Some(RuntimeTasks {
                    ingest,
                    cleanup,
                    incident_relay,
                    persistence_runtime,
                })),
            }),
        })
    }

    pub fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<LiveBroadcastEvent> {
        self.inner.state.event_tx.subscribe()
    }

    pub fn subscribe_incident_changes(
        &self,
    ) -> Option<tokio::sync::broadcast::Receiver<IncidentBroadcastEvent>> {
        self.inner
            .state
            .archive
            .as_ref()
            .map(|_| self.inner.state.incident_event_tx.subscribe())
    }

    pub fn telemetry_snapshot(&self) -> LiveTelemetry {
        self.inner
            .state
            .telemetry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn stats_snapshot(&self) -> LiveStatsSnapshot {
        let state = &self.inner.state;
        let upstream_endpoint = state
            .upstream_endpoint
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();

        LiveStatsSnapshot {
            uptime_secs: state.started_at.elapsed().as_secs(),
            data_blocks_total: state.data_blocks_total.load(Ordering::Relaxed),
            received_servers: state.received_servers.load(Ordering::Relaxed),
            received_sat_servers: state.received_sat_servers.load(Ordering::Relaxed),
            retained_files: state
                .retained_files
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
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

    pub fn list_retained_files(&self) -> Vec<emwin_service::CompletedFileMetadata> {
        self.inner
            .state
            .retained_files
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .list()
    }

    pub fn get_retained_file(&self, filename: &str) -> Option<RetainedFile> {
        self.inner
            .state
            .retained_files
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(filename)
    }

    pub async fn shutdown(&self) -> LiveResult<()> {
        let mut guard = self.inner.tasks.lock().await;
        let Some(tasks) = guard.take() else {
            return Ok(());
        };

        let _ = self.inner.shutdown_tx.send(true);

        await_task(tasks.ingest, "ingest").await?;
        if let Some(task) = tasks.cleanup {
            await_task(task, "cleanup").await?;
        }
        if let Some(task) = tasks.incident_relay {
            await_task(task, "incident relay").await?;
        }
        if let Some(runtime) = tasks.persistence_runtime {
            let _ = shutdown_runtime(runtime).await?;
        }

        Ok(())
    }

    pub fn new_for_tests(
        retained_files: Vec<(String, Vec<u8>, u64, SourceKind)>,
        telemetry: crate::types::LiveTelemetry,
        archive: Option<emwin_db::PostgresMetadataSink>,
        persistence: Option<emwin_db::PersistenceProducer<emwin_db::CompletedFileMetadata>>,
        upstream_endpoint: Option<String>,
    ) -> Self {
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);
        let state = AppState::new(persistence, archive, true, 32, 60);
        {
            let mut retained = state
                .retained_files
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
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
                    data,
                    timestamp_utc,
                    origin,
                    std::time::SystemTime::now(),
                );
            }
        }
        {
            let mut guard = state
                .telemetry
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *guard = telemetry;
        }
        {
            let mut guard = state
                .upstream_endpoint
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *guard = upstream_endpoint;
        }

        Self {
            inner: Arc::new(LiveRuntimeInner {
                state,
                shutdown_tx,
                tasks: Mutex::new(None),
            }),
        }
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

impl LiveEventService for LiveRuntime {
    fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<LiveBroadcastEvent> {
        LiveRuntime::subscribe_events(self)
    }

    fn telemetry_snapshot(&self) -> LiveTelemetry {
        LiveRuntime::telemetry_snapshot(self)
    }

    fn stats_snapshot(&self) -> LiveStatsSnapshot {
        LiveRuntime::stats_snapshot(self)
    }

    fn shutdown(&self) -> emwin_service::archive::BoxFuture<'_, ServiceResult<()>> {
        Box::pin(async move {
            LiveRuntime::shutdown(self)
                .await
                .map_err(|err| ServiceError::Runtime(err.to_string()))
        })
    }
}

impl RetainedFileService for LiveRuntime {
    fn list_retained_files(&self) -> Vec<emwin_service::CompletedFileMetadata> {
        LiveRuntime::list_retained_files(self)
    }

    fn get_retained_file(&self, filename: &str) -> Option<RetainedFile> {
        LiveRuntime::get_retained_file(self, filename)
    }
}

impl IncidentChangeStream for LiveRuntime {
    fn subscribe_incident_changes(
        &self,
    ) -> Option<tokio::sync::broadcast::Receiver<IncidentBroadcastEvent>> {
        LiveRuntime::subscribe_incident_changes(self)
    }
}

impl ArchiveQueryService for LiveRuntime {
    fn list_incidents(
        &self,
        query: IncidentListQuery,
    ) -> emwin_service::archive::BoxFuture<'_, ServiceResult<PaginatedResponse<IncidentSummary>>>
    {
        Box::pin(async move {
            let archive = self.inner.state.archive.as_ref().ok_or_else(|| {
                ServiceError::NotConfigured("archive database is not configured".to_string())
            })?;
            archive
                .list_incidents(query)
                .await
                .map_err(map_service_error)
        })
    }

    fn get_incident<'a>(
        &'a self,
        key: &'a IncidentKey,
    ) -> emwin_service::archive::BoxFuture<'a, ServiceResult<Option<IncidentDetail>>> {
        Box::pin(async move {
            let archive = self.inner.state.archive.as_ref().ok_or_else(|| {
                ServiceError::NotConfigured("archive database is not configured".to_string())
            })?;
            archive.get_incident(key).await.map_err(map_service_error)
        })
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
            let archive = self.inner.state.archive.as_ref().ok_or_else(|| {
                ServiceError::NotConfigured("archive database is not configured".to_string())
            })?;
            archive
                .list_incident_products(key, query)
                .await
                .map_err(map_service_error)
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
            let archive = self.inner.state.archive.as_ref().ok_or_else(|| {
                ServiceError::NotConfigured("archive database is not configured".to_string())
            })?;
            archive
                .list_archived_products(query)
                .await
                .map_err(map_service_error)
        })
    }

    fn get_archived_product(
        &self,
        product_id: i64,
    ) -> emwin_service::archive::BoxFuture<'_, ServiceResult<Option<ArchivedProductDetail>>> {
        Box::pin(async move {
            let archive = self.inner.state.archive.as_ref().ok_or_else(|| {
                ServiceError::NotConfigured("archive database is not configured".to_string())
            })?;
            archive
                .get_archived_product(product_id)
                .await
                .map_err(map_service_error)
        })
    }

    fn list_archived_issues(
        &self,
        query: ArchivedIssueListQuery,
    ) -> emwin_service::archive::BoxFuture<'_, ServiceResult<PaginatedResponse<ArchivedIssue>>>
    {
        Box::pin(async move {
            let archive = self.inner.state.archive.as_ref().ok_or_else(|| {
                ServiceError::NotConfigured("archive database is not configured".to_string())
            })?;
            archive
                .list_archived_issues(query)
                .await
                .map_err(map_service_error)
        })
    }

    fn get_archived_issue(
        &self,
        issue_id: i64,
    ) -> emwin_service::archive::BoxFuture<'_, ServiceResult<Option<ArchivedIssue>>> {
        Box::pin(async move {
            let archive = self.inner.state.archive.as_ref().ok_or_else(|| {
                ServiceError::NotConfigured("archive database is not configured".to_string())
            })?;
            archive
                .get_archived_issue(issue_id)
                .await
                .map_err(map_service_error)
        })
    }

    fn read_archived_payload(
        &self,
        product_id: i64,
    ) -> emwin_service::archive::BoxFuture<'_, ServiceResult<Option<ArchivedPayload>>> {
        Box::pin(async move {
            let archive = self.inner.state.archive.as_ref().ok_or_else(|| {
                ServiceError::NotConfigured("archive database is not configured".to_string())
            })?;
            archive
                .read_archived_payload(product_id)
                .await
                .map_err(map_service_error)
        })
    }

    fn list_archived_features(
        &self,
        query: FeatureListQuery,
    ) -> emwin_service::archive::BoxFuture<'_, ServiceResult<PaginatedResponse<ArchivedFeature>>>
    {
        Box::pin(async move {
            let archive = self.inner.state.archive.as_ref().ok_or_else(|| {
                ServiceError::NotConfigured("archive database is not configured".to_string())
            })?;
            archive
                .list_archived_features(query)
                .await
                .map_err(map_service_error)
        })
    }

    fn list_facet_aggregate(
        &self,
        query: FacetAggregateQuery,
    ) -> emwin_service::archive::BoxFuture<'_, ServiceResult<FacetAggregateResult>> {
        Box::pin(async move {
            let archive = self.inner.state.archive.as_ref().ok_or_else(|| {
                ServiceError::NotConfigured("archive database is not configured".to_string())
            })?;
            archive
                .list_facet_aggregate(query)
                .await
                .map_err(map_service_error)
        })
    }

    fn list_timeseries_aggregate(
        &self,
        query: TimeseriesAggregateQuery,
    ) -> emwin_service::archive::BoxFuture<'_, ServiceResult<TimeseriesAggregateResult>> {
        Box::pin(async move {
            let archive = self.inner.state.archive.as_ref().ok_or_else(|| {
                ServiceError::NotConfigured("archive database is not configured".to_string())
            })?;
            archive
                .list_timeseries_aggregate(query)
                .await
                .map_err(map_service_error)
        })
    }

    fn list_cell_aggregate(
        &self,
        query: CellAggregateQuery,
    ) -> emwin_service::archive::BoxFuture<'_, ServiceResult<CellAggregateResult>> {
        Box::pin(async move {
            let archive = self.inner.state.archive.as_ref().ok_or_else(|| {
                ServiceError::NotConfigured("archive database is not configured".to_string())
            })?;
            archive
                .list_cell_aggregate(query)
                .await
                .map_err(map_service_error)
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
