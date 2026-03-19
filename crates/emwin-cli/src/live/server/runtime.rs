use super::server_http;
use super::server_ingest;
use super::types::{AppState, ServerOptions, TelemetryPayload};
use crate::ReceiverKind;
use crate::live::config::{LiveConfigRequest, LiveReceiverConfig, build_live_receiver_config};
use crate::live::persistence::{run_incident_cleanup_loop, start_runtime_with_postgres};
use crate::live::server_support::RetainedFiles;
use chrono::Utc;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, watch};

const EVENT_CHANNEL_CAPACITY: usize = 4096;

pub async fn run(options: ServerOptions) -> crate::error::CliResult<()> {
    let ServerOptions {
        receiver,
        username,
        password,
        raw_servers,
        server_list_path,
        output_dir,
        bind,
        cors_origin,
        max_clients,
        stats_interval_secs,
        file_retention_secs,
        max_retained_files,
        post_process_archives,
        quiet,
        persistence_queue_capacity,
        postgres_database_url,
        openapi_auth_token,
    } = options;

    if postgres_database_url.is_some() && output_dir.is_none() {
        return Err(crate::error::CliError::invalid_argument(
            "--persist-database-url requires --output-dir for blob storage",
        ));
    }
    if openapi_auth_token
        .as_deref()
        .is_some_and(|token| token.trim().is_empty())
    {
        return Err(crate::error::CliError::invalid_argument(
            "--openapi-auth-token must not be empty",
        ));
    }

    let bind_addr = SocketAddr::from_str(&bind).map_err(|err| {
        crate::error::CliError::invalid_argument(format!("invalid --bind value {bind}: {err}"))
    })?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let started_persistence = match output_dir {
        Some(path) => Some(
            start_runtime_with_postgres(
                path,
                persistence_queue_capacity,
                postgres_database_url.as_deref(),
                "emwin-cli-server",
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

    let state = Arc::new(AppState {
        event_tx: broadcast::channel(EVENT_CHANNEL_CAPACITY).0,
        incident_event_tx: broadcast::channel(EVENT_CHANNEL_CAPACITY).0,
        shutdown_rx: shutdown_rx.clone(),
        retained_files: Mutex::new(RetainedFiles::new(
            max_retained_files.max(1),
            Duration::from_secs(file_retention_secs.max(1)),
        )),
        telemetry: Mutex::new(TelemetryPayload::Unavailable),
        persistence: persistence_producer.clone(),
        archive: archive_sink.clone(),
        connected_clients: AtomicUsize::new(0),
        max_clients: max_clients.max(1),
        next_event_id: AtomicU64::new(1),
        next_incident_event_id: AtomicU64::new(1),
        data_blocks_total: AtomicU64::new(0),
        received_servers: AtomicUsize::new(0),
        received_sat_servers: AtomicUsize::new(0),
        started_at: Instant::now(),
        upstream_endpoint: Mutex::new(None),
        openapi_auth_token,
        quiet,
    });

    let cors = super::build_cors_layer(cors_origin)?;
    let app = server_http::build_router(Arc::clone(&state), cors);

    let listener = TcpListener::bind(bind_addr).await?;
    super::log_info(quiet, &format!("server listening addr={bind_addr}"));

    let incident_relay_task = archive_sink.clone().map(|sink| {
        tokio::spawn(server_ingest::run_incident_event_relay_loop(
            sink,
            Arc::clone(&state),
            shutdown_rx.clone(),
        ))
    });

    if let Some(postgres_sink) = archive_sink.as_ref() {
        match postgres_sink.expire_active_incidents(Utc::now()).await {
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

    let ingest_task = match receiver {
        ReceiverKind::Qbt => {
            let LiveReceiverConfig::Qbt(config) = build_live_receiver_config(LiveConfigRequest {
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
                unreachable!("qbt server mode must build qbt config");
            };
            tokio::spawn(server_ingest::run_qbt_ingest_loop(
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
                unreachable!("wxwire server mode must build wxwire config");
            };
            tokio::spawn(server_ingest::run_wxwire_ingest_loop(
                config,
                Arc::clone(&state),
                post_process_archives,
                persistence_producer.clone(),
                shutdown_rx.clone(),
            ))
        }
    };
    let stats_task = tokio::spawn(server_ingest::run_stats_loop(
        Arc::clone(&state),
        stats_interval_secs,
        persistence_producer.clone(),
        shutdown_rx.clone(),
    ));
    let cleanup_task =
        cleanup_sink.map(|sink| tokio::spawn(run_incident_cleanup_loop(sink, shutdown_rx.clone())));
    let mut http_shutdown_rx = shutdown_rx.clone();

    let serve = async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = http_shutdown_rx.changed().await;
        })
        .await
    };
    tokio::pin!(serve);

    let mut shutdown_requested = false;
    let serve_result = tokio::select! {
        result = &mut serve => result,
        signal = tokio::signal::ctrl_c() => {
            signal?;
            shutdown_requested = true;
            tracing::info!("shutdown signal received; waiting for server tasks to stop");
            let _ = shutdown_tx.send(true);
            (&mut serve).await
        }
    };

    if !shutdown_requested {
        let _ = shutdown_tx.send(true);
    }

    let ingest_result = ingest_task.await;
    let stats_result = stats_task.await;
    let cleanup_result = match cleanup_task {
        Some(task) => Some(task.await),
        None => None,
    };
    let incident_relay_result = match incident_relay_task {
        Some(task) => Some(task.await),
        None => None,
    };
    let _persistence_shutdown_stats = match persistence_runtime {
        Some(runtime) => Some(crate::live::persistence::shutdown_runtime(runtime).await?),
        None => None,
    };

    if let Err(err) = serve_result {
        return Err(crate::error::CliError::runtime(format!(
            "http server failed: {err}"
        )));
    }
    match ingest_result {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            return Err(crate::error::CliError::runtime(format!(
                "ingest task failed: {err}"
            )));
        }
        Err(err) => {
            return Err(crate::error::CliError::runtime(format!(
                "ingest task join failed: {err}"
            )));
        }
    }
    match stats_result {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            return Err(crate::error::CliError::runtime(format!(
                "stats task failed: {err}"
            )));
        }
        Err(err) => {
            return Err(crate::error::CliError::runtime(format!(
                "stats task join failed: {err}"
            )));
        }
    }
    match cleanup_result {
        Some(Ok(Ok(()))) | None => {}
        Some(Ok(Err(err))) => {
            return Err(crate::error::CliError::runtime(format!(
                "cleanup task failed: {err}"
            )));
        }
        Some(Err(err)) => {
            return Err(crate::error::CliError::runtime(format!(
                "cleanup task join failed: {err}"
            )));
        }
    }
    match incident_relay_result {
        Some(Ok(Ok(()))) | None => {}
        Some(Ok(Err(err))) => {
            return Err(crate::error::CliError::runtime(format!(
                "incident relay task failed: {err}"
            )));
        }
        Some(Err(err)) => {
            return Err(crate::error::CliError::runtime(format!(
                "incident relay task join failed: {err}"
            )));
        }
    }
    tracing::info!("server stopped");
    Ok(())
}
