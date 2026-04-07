use super::server_http;
use super::types::{AppState, EventKind, HttpServerOptions, IncidentEventPayload};
use crate::error::{ApiError, ApiResult};
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, watch};

const EVENT_CHANNEL_CAPACITY: usize = 4096;

pub async fn serve(options: HttpServerOptions, live: emwin_live::LiveRuntime) -> ApiResult<()> {
    let HttpServerOptions {
        bind,
        cors_origin,
        max_clients,
        stats_interval_secs,
        quiet,
        openapi_auth_token,
    } = options;

    if openapi_auth_token
        .as_deref()
        .is_some_and(|token| token.trim().is_empty())
    {
        return Err(ApiError::invalid_argument(
            "--openapi-auth-token must not be empty",
        ));
    }

    let bind_addr = SocketAddr::from_str(&bind)
        .map_err(|err| ApiError::invalid_argument(format!("invalid --bind value {bind}: {err}")))?;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let state = Arc::new(AppState {
        live: live.clone(),
        event_tx: broadcast::channel(EVENT_CHANNEL_CAPACITY).0,
        incident_event_tx: broadcast::channel(EVENT_CHANNEL_CAPACITY).0,
        shutdown_rx: shutdown_rx.clone(),
        connected_clients: AtomicUsize::new(0),
        max_clients: max_clients.max(1),
        next_event_id: AtomicU64::new(1),
        next_incident_event_id: AtomicU64::new(1),
        openapi_auth_token,
        quiet,
    });

    let cors = super::build_cors_layer(cors_origin)?;
    let app = server_http::build_router(Arc::clone(&state), cors);
    let listener = TcpListener::bind(bind_addr).await?;
    super::log_info(quiet, &format!("server listening addr={bind_addr}"));

    let event_relay_task = tokio::spawn(run_event_relay_loop(
        live.subscribe_events(),
        Arc::clone(&state),
        shutdown_rx.clone(),
    ));
    let incident_relay_task = live.subscribe_incident_changes().map(|rx| {
        tokio::spawn(run_incident_relay_loop(
            rx,
            Arc::clone(&state),
            shutdown_rx.clone(),
        ))
    });
    let stats_task = tokio::spawn(run_stats_loop(
        live.clone(),
        Arc::clone(&state),
        stats_interval_secs,
        shutdown_rx.clone(),
    ));

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

    await_task(event_relay_task, "event relay").await?;
    if let Some(task) = incident_relay_task {
        await_task(task, "incident relay").await?;
    }
    await_task(stats_task, "stats").await?;
    live.shutdown().await?;

    if let Err(err) = serve_result {
        return Err(ApiError::runtime(format!("http server failed: {err}")));
    }

    tracing::info!("server stopped");
    Ok(())
}

async fn run_event_relay_loop(
    mut rx: broadcast::Receiver<emwin_live::LiveBroadcastEvent>,
    state: Arc<AppState>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> ApiResult<()> {
    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => break,
            received = rx.recv() => match received {
                Ok(event) => super::publish(&state, map_live_event(event.kind)),
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(dropped)) => {
                    super::log_info(state.quiet, &format!("event relay lagged dropped={dropped}"));
                }
            }
        }
    }
    Ok(())
}

async fn run_incident_relay_loop(
    mut rx: broadcast::Receiver<emwin_live::IncidentBroadcastEvent>,
    state: Arc<AppState>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> ApiResult<()> {
    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => break,
            received = rx.recv() => match received {
                Ok(event) => super::publish_incident_change(
                    &state,
                    IncidentEventPayload::from_change(event.change),
                ),
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(dropped)) => {
                    super::log_info(state.quiet, &format!("incident relay lagged dropped={dropped}"));
                }
            }
        }
    }
    Ok(())
}

async fn run_stats_loop(
    live: emwin_live::LiveRuntime,
    state: Arc<AppState>,
    stats_interval_secs: u64,
    mut shutdown_rx: watch::Receiver<bool>,
) -> ApiResult<()> {
    if stats_interval_secs == 0 {
        let _ = shutdown_rx.changed().await;
        return Ok(());
    }

    let mut interval =
        tokio::time::interval(std::time::Duration::from_secs(stats_interval_secs.max(1)));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => break,
            _ = interval.tick() => {
                if state.quiet {
                    continue;
                }

                let snapshot = live.stats_snapshot();
                let connected_clients = state.connected_clients.load(Ordering::Relaxed);
                if let Some(persistence) = snapshot.persistence {
                    tracing::info!(
                        uptime_secs = snapshot.uptime_secs,
                        data_blocks_total = snapshot.data_blocks_total,
                        received_servers = snapshot.received_servers,
                        received_sat_servers = snapshot.received_sat_servers,
                        retained_files = snapshot.retained_files,
                        connected_clients,
                        upstream = snapshot.upstream_endpoint.as_deref().unwrap_or("disconnected"),
                        persistence_queue_len = persistence.queue_len,
                        persistence_queue_capacity = persistence.queue_capacity,
                        persistence_enqueued_total = persistence.enqueued_total,
                        persistence_evicted_total = persistence.evicted_total,
                        persistence_persisted_total = persistence.persisted_total,
                        persistence_failed_total = persistence.failed_total,
                        "server stats snapshot"
                    );
                } else {
                    tracing::info!(
                        uptime_secs = snapshot.uptime_secs,
                        data_blocks_total = snapshot.data_blocks_total,
                        received_servers = snapshot.received_servers,
                        received_sat_servers = snapshot.received_sat_servers,
                        retained_files = snapshot.retained_files,
                        connected_clients,
                        upstream = snapshot.upstream_endpoint.as_deref().unwrap_or("disconnected"),
                        "server stats snapshot"
                    );
                }
            }
        }
    }

    Ok(())
}

fn map_live_event(kind: emwin_live::LiveEventKind) -> EventKind {
    match kind {
        emwin_live::LiveEventKind::Connected { endpoint } => EventKind::Connected { endpoint },
        emwin_live::LiveEventKind::Disconnected => EventKind::Disconnected,
        emwin_live::LiveEventKind::ReceiverFrame(frame) => EventKind::ReceiverFrame(frame),
        emwin_live::LiveEventKind::ProductAvailable(metadata) => EventKind::FileComplete(Box::new(
            super::types::CompletedFileEventPayload::from_metadata(*metadata),
        )),
        emwin_live::LiveEventKind::Telemetry(value) => EventKind::Telemetry(value),
        emwin_live::LiveEventKind::Error { message } => EventKind::Error { message },
    }
}

async fn await_task(task: tokio::task::JoinHandle<ApiResult<()>>, name: &str) -> ApiResult<()> {
    match task.await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => Err(ApiError::runtime(format!("{name} task failed: {err}"))),
        Err(err) => Err(ApiError::runtime(format!("{name} task join failed: {err}"))),
    }
}
