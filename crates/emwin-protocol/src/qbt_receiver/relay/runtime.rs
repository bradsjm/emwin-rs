//! QBT relay runtime implementation.
//!
//! This module implements the relay server that accepts downstream client connections
//! and forwards upstream QBT data. It handles client authentication, quality monitoring,
//! metrics logging, and graceful shutdown.

use super::auth::AuthParser;
use super::state::QbtRelayState;
use super::{QbtRelayConfig, QbtRelayError, QbtRelayResult};
use crate::qbt_receiver::client::connection::connect_with_timeout;
use crate::qbt_receiver::protocol::auth::{REAUTH_INTERVAL_SECS, build_logon_message, xor_ff};
use crate::qbt_receiver::protocol::server_list_wire::QbtServerListWireScanner;
use crate::runtime_support::lock_unpoisoned;
use bytes::Bytes;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, broadcast, mpsc, watch};
use tracing::{debug, error, info, warn};

const INITIAL_AUTH_TIMEOUT_SECS: u64 = 30;
const UPSTREAM_READ_BUFFER_BYTES: usize = 8192;
const UPSTREAM_CHANNEL_CAPACITY: usize = 4096;
const CLIENT_WRITER_QUEUE_CAPACITY: usize = 1024;
const QUALITY_RESUME_THRESHOLD: f64 = 0.97;

struct QueuedChunk {
    bytes: Bytes,
    _permit: OwnedSemaphorePermit,
}

/// Runs the QBT relay accept loop and supervisory tasks.
pub async fn run(
    config: QbtRelayConfig,
    state: Arc<QbtRelayState>,
    shutdown_rx: watch::Receiver<bool>,
) -> QbtRelayResult<()> {
    let config = config.normalized();
    config.validate()?;

    info!(
        relay_bind = %config.bind_addr,
        max_clients = config.max_clients,
        auth_timeout_secs = config.auth_timeout.as_secs(),
        client_buffer_bytes = config.client_buffer_bytes,
        quality_window_secs = config.quality_window_secs,
        quality_pause_threshold = config.quality_pause_threshold,
        metrics_log_interval_secs = config.metrics_log_interval.as_secs(),
        upstream_server_count = config.upstream_servers.len(),
        "relay runtime starting"
    );

    let (upstream_tx, _) = broadcast::channel::<Bytes>(UPSTREAM_CHANNEL_CAPACITY);

    let upstream_state = Arc::clone(&state);
    let upstream_config = config.clone();
    let upstream_shutdown = shutdown_rx.clone();
    let upstream_sender = upstream_tx.clone();
    let upstream_task = tokio::spawn(async move {
        run_upstream_loop(
            upstream_state,
            upstream_config,
            upstream_sender,
            upstream_shutdown,
        )
        .await;
    });

    let accept_state = Arc::clone(&state);
    let accept_config = config.clone();
    let accept_shutdown = shutdown_rx.clone();
    let accept_task = tokio::spawn(async move {
        run_accept_loop(accept_state, accept_config, upstream_tx, accept_shutdown).await
    });

    let quality_state = Arc::clone(&state);
    let quality_config = config.clone();
    let quality_shutdown = shutdown_rx.clone();
    let quality_task = tokio::spawn(async move {
        run_quality_monitor(quality_state, quality_config, quality_shutdown).await;
    });

    let metrics_log_state = Arc::clone(&state);
    let metrics_log_config = config;
    let metrics_log_shutdown = shutdown_rx;
    let metrics_log_task = tokio::spawn(async move {
        run_metrics_logger(metrics_log_state, metrics_log_config, metrics_log_shutdown).await;
    });

    if let Err(join_err) = upstream_task.await {
        error!(error = %join_err, "upstream task join failed");
        return Err(QbtRelayError::TaskJoin(join_err.to_string()));
    }
    match accept_task.await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => {
            error!(error = %err, "accept loop failed");
            return Err(err);
        }
        Err(join_err) => {
            error!(error = %join_err, "accept task join failed");
            return Err(QbtRelayError::TaskJoin(join_err.to_string()));
        }
    }
    if let Err(join_err) = quality_task.await {
        error!(error = %join_err, "quality task join failed");
        return Err(QbtRelayError::TaskJoin(join_err.to_string()));
    }
    if let Err(join_err) = metrics_log_task.await {
        error!(error = %join_err, "metrics log task join failed");
        return Err(QbtRelayError::TaskJoin(join_err.to_string()));
    }

    info!("relay runtime stopped");
    Ok(())
}

/// Runs the metrics logging loop, periodically emitting relay statistics.
async fn run_metrics_logger(
    state: Arc<QbtRelayState>,
    config: QbtRelayConfig,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut interval = tokio::time::interval(config.metrics_log_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    interval.tick().await;

    while !*shutdown_rx.borrow() {
        tokio::select! {
            _ = shutdown_rx.changed() => return,
            _ = interval.tick() => {
                let rolling_quality = state.metrics.rolling_quality_milli.load(Ordering::Relaxed) as f64 / 1000.0;
                info!(
                    active_clients = state.metrics.downstream_active_clients.load(Ordering::Relaxed),
                    bytes_in_total = state.metrics.bytes_in_total.load(Ordering::Relaxed),
                    bytes_attempted_total = state.metrics.bytes_attempted_total.load(Ordering::Relaxed),
                    bytes_forwarded_total = state.metrics.bytes_forwarded_total.load(Ordering::Relaxed),
                    bytes_dropped_total = state.metrics.bytes_dropped_total.load(Ordering::Relaxed),
                    upstream_connection_attempts_total = state.metrics.upstream_connection_attempts_total.load(Ordering::Relaxed),
                    upstream_connection_success_total = state.metrics.upstream_connection_success_total.load(Ordering::Relaxed),
                    upstream_connection_fail_total = state.metrics.upstream_connection_fail_total.load(Ordering::Relaxed),
                    upstream_disconnect_total = state.metrics.upstream_disconnect_total.load(Ordering::Relaxed),
                    forwarding_paused = state.metrics.forwarding_paused.load(Ordering::Relaxed),
                    rolling_quality,
                    "relay metrics snapshot"
                );
            }
        }
    }
}

/// Runs the upstream connection loop, maintaining connection to QBT servers.
///
/// Connects to upstream servers in rotation, handles authentication,
/// and forwards received data to downstream clients.
async fn run_upstream_loop(
    state: Arc<QbtRelayState>,
    config: QbtRelayConfig,
    upstream_tx: broadcast::Sender<Bytes>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut server_index = 0usize;
    let mut scanner = QbtServerListWireScanner::default();
    let mut read_buf = vec![0u8; UPSTREAM_READ_BUFFER_BYTES];

    while !*shutdown_rx.borrow() {
        let endpoint = &config.upstream_servers[server_index % config.upstream_servers.len()];
        server_index = (server_index + 1) % config.upstream_servers.len();
        info!(endpoint = %format!("{}:{}", endpoint.0, endpoint.1), "connecting upstream");

        state
            .metrics
            .upstream_connection_attempts_total
            .fetch_add(1, Ordering::Relaxed);

        let connect =
            connect_with_timeout(endpoint.0.as_str(), endpoint.1, config.connect_timeout).await;

        let Ok(mut stream) = connect else {
            state
                .metrics
                .upstream_connection_fail_total
                .fetch_add(1, Ordering::Relaxed);
            warn!(
                endpoint = %format!("{}:{}", endpoint.0, endpoint.1),
                reconnect_delay_secs = config.reconnect_delay.as_secs(),
                "upstream connection failed"
            );
            tokio::select! {
                _ = tokio::time::sleep(config.reconnect_delay) => {}
                _ = shutdown_rx.changed() => {}
            }
            continue;
        };

        state
            .metrics
            .upstream_connection_success_total
            .fetch_add(1, Ordering::Relaxed);
        info!(endpoint = %format!("{}:{}", endpoint.0, endpoint.1), "upstream connected");

        let initial_auth = xor_ff(build_logon_message(&config.email).as_bytes());
        if let Err(err) = stream.write_all(&initial_auth).await {
            state
                .metrics
                .upstream_disconnect_total
                .fetch_add(1, Ordering::Relaxed);
            warn!(error = %err, "upstream disconnected while sending initial auth");
            continue;
        }

        let mut auth_interval = tokio::time::interval(Duration::from_secs(REAUTH_INTERVAL_SECS));
        auth_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        auth_interval.tick().await;

        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    return;
                }
                _ = auth_interval.tick() => {
                    let auth = xor_ff(build_logon_message(&config.email).as_bytes());
                    if let Err(err) = stream.write_all(&auth).await {
                        state.metrics.upstream_disconnect_total.fetch_add(1, Ordering::Relaxed);
                        warn!(error = %err, "upstream disconnected during periodic re-auth");
                        break;
                    }
                }
                read = stream.read(&mut read_buf) => {
                    match read {
                        Ok(0) => {
                            state.metrics.upstream_disconnect_total.fetch_add(1, Ordering::Relaxed);
                            warn!("upstream closed connection");
                            break;
                        }
                        Ok(n) => {
                            let bytes = Bytes::copy_from_slice(&read_buf[..n]);
                            state.metrics.bytes_in_total.fetch_add(n as u64, Ordering::Relaxed);
                            if let Some(server_list_wire) = scanner.observe_wire_chunk(&bytes) {
                                state.set_latest_server_list_wire(server_list_wire);
                                info!("updated cached upstream server list frame");
                            }
                            let _ = upstream_tx.send(bytes);
                        }
                        Err(err) => {
                            state.metrics.upstream_disconnect_total.fetch_add(1, Ordering::Relaxed);
                            warn!(error = %err, "upstream read failed");
                            break;
                        }
                    }
                }
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(config.reconnect_delay) => {}
            _ = shutdown_rx.changed() => { return; }
        }
    }
}

/// Runs the TCP accept loop, accepting downstream client connections.
///
/// Accepts connections up to max_clients limit, handles capacity rejection,
/// and spawns client session tasks for authenticated clients.
async fn run_accept_loop(
    state: Arc<QbtRelayState>,
    config: QbtRelayConfig,
    upstream_tx: broadcast::Sender<Bytes>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> QbtRelayResult<()> {
    let listener = TcpListener::bind(config.bind_addr).await?;
    info!(listen_addr = %config.bind_addr, "downstream listener ready");

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => return Ok(()),
            accept = listener.accept() => {
                let (stream, peer) = accept?;
                if state.metrics.downstream_active_clients
                    .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                        (current < config.max_clients as u64).then_some(current + 1)
                    })
                    .is_err()
                {
                    state.metrics.downstream_connections_rejected_over_capacity_total.fetch_add(1, Ordering::Relaxed);
                    warn!(peer = %peer, max_clients = config.max_clients, "rejecting downstream client over capacity");
                    let mut socket = stream;
                    let server_list = state.latest_server_list_wire();
                    if let Err(err) = socket.write_all(&server_list).await {
                        debug!(peer = %peer, error = %err, "failed sending server list to rejected client");
                    }
                    if let Err(err) = socket.shutdown().await {
                        debug!(peer = %peer, error = %err, "failed shutting down rejected client socket");
                    }
                    continue;
                }

                state.metrics.downstream_connections_accepted_total.fetch_add(1, Ordering::Relaxed);
                let client_id = state.next_client_id.fetch_add(1, Ordering::Relaxed);
                info!(client_id, peer = %peer, "accepted downstream client");
                let client_state = Arc::clone(&state);
                let mut client_shutdown = shutdown_rx.clone();
                let client_rx = upstream_tx.subscribe();
                let client_config = config.clone();
                tokio::spawn(async move {
                    let _ = run_client_session(client_state, client_config, client_id, stream, peer, client_rx, &mut client_shutdown).await;
                });
            }
        }
    }
}

/// Manages a single downstream client session.
///
/// Handles authentication, monitors for re-auth timeouts, and forwards
/// upstream data to the client. Disconnects slow or unresponsive clients.
async fn run_client_session(
    state: Arc<QbtRelayState>,
    config: QbtRelayConfig,
    client_id: u64,
    stream: TcpStream,
    peer: std::net::SocketAddr,
    mut upstream_rx: broadcast::Receiver<Bytes>,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> QbtRelayResult<()> {
    let (mut reader, writer) = stream.into_split();
    let queue_permits = Arc::new(Semaphore::new(config.client_buffer_bytes));
    let (writer_tx, writer_rx) = mpsc::channel::<QueuedChunk>(CLIENT_WRITER_QUEUE_CAPACITY);

    let writer_state = Arc::clone(&state);
    let writer_task =
        tokio::spawn(async move { run_client_writer(writer_state, writer, writer_rx).await });

    let mut auth_parser = AuthParser::default();
    let mut read_buf = vec![0u8; 2048];
    let connected_at = Instant::now();
    let mut last_auth = Instant::now();
    let mut email = String::new();
    let mut is_authenticated = false;
    let mut disconnect_reason = "client_closed";
    let mut read_poll_timeouts = 0u64;

    loop {
        if *shutdown_rx.borrow() {
            break;
        }

        if !is_authenticated
            && connected_at.elapsed() >= Duration::from_secs(INITIAL_AUTH_TIMEOUT_SECS)
        {
            state
                .metrics
                .downstream_disconnect_auth_timeout_total
                .fetch_add(1, Ordering::Relaxed);
            disconnect_reason = "initial_auth_timeout";
            warn!(client_id, peer = %peer, timeout_secs = INITIAL_AUTH_TIMEOUT_SECS, "downstream client did not authenticate in time");
            break;
        }

        if is_authenticated && last_auth.elapsed() >= config.auth_timeout {
            state
                .metrics
                .downstream_disconnect_auth_timeout_total
                .fetch_add(1, Ordering::Relaxed);
            disconnect_reason = "reauth_timeout";
            warn!(client_id, peer = %peer, auth_timeout_secs = config.auth_timeout.as_secs(), "downstream client re-auth timeout");
            break;
        }

        tokio::select! {
            _ = shutdown_rx.changed() => {
                disconnect_reason = "shutdown";
                break;
            }
            read = tokio::time::timeout(Duration::from_secs(1), reader.read(&mut read_buf)) => {
                match read {
                    Ok(Ok(0)) => {
                        disconnect_reason = "client_closed";
                        break;
                    }
                    Ok(Ok(n)) => {
                        if let Some(found) = auth_parser.consume(&read_buf[..n]) {
                            email = found;
                            last_auth = Instant::now();
                            is_authenticated = true;
                            let now_secs = unix_time_secs();
                            let mut clients = lock_unpoisoned(&state.clients);
                            clients.insert(client_id, super::state::QbtRelayClientMeta {
                                email: email.clone(),
                                peer: peer.to_string(),
                                connected_at_unix_secs: now_secs,
                                last_auth_unix_secs: now_secs,
                            });
                            info!(client_id, peer = %peer, user = %email, "downstream client authenticated");
                        }
                    }
                    Ok(Err(err)) => {
                        disconnect_reason = "client_read_error";
                        debug!(client_id, peer = %peer, error = %err, "downstream client read error");
                        break;
                    }
                    Err(_) => {
                        read_poll_timeouts = read_poll_timeouts.saturating_add(1);
                    }
                }
            }
            recv = upstream_rx.recv(), if is_authenticated => {
                match recv {
                    Ok(chunk) => {
                        if state.metrics.forwarding_paused.load(Ordering::Relaxed) {
                            continue;
                        }
                        let len = chunk.len() as u64;
                        state.add_attempted(len);

                        let permit = queue_permits
                            .clone()
                            .try_acquire_many_owned(chunk.len() as u32);
                        let Ok(permit) = permit else {
                            state.metrics.downstream_disconnect_slow_client_total.fetch_add(1, Ordering::Relaxed);
                            state.metrics.bytes_dropped_total.fetch_add(len, Ordering::Relaxed);
                            disconnect_reason = "slow_client_buffer_exceeded";
                            warn!(client_id, peer = %peer, queued_limit_bytes = config.client_buffer_bytes, dropped_bytes = len, "disconnecting slow downstream client");
                            break;
                        };

                        let queued = QueuedChunk { bytes: chunk, _permit: permit };
                        match writer_tx.try_send(queued) {
                            Ok(()) => {}
                            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                                disconnect_reason = "writer_channel_closed";
                                debug!(client_id, peer = %peer, "downstream writer channel closed");
                                break;
                            }
                            Err(tokio::sync::mpsc::error::TrySendError::Full(item)) => {
                                state.metrics.downstream_disconnect_slow_client_total.fetch_add(1, Ordering::Relaxed);
                                state.metrics.bytes_dropped_total.fetch_add(item.bytes.len() as u64, Ordering::Relaxed);
                                disconnect_reason = "writer_queue_full";
                                warn!(
                                    client_id,
                                    peer = %peer,
                                    queue_capacity = CLIENT_WRITER_QUEUE_CAPACITY,
                                    queued_limit_bytes = config.client_buffer_bytes,
                                    dropped_bytes = item.bytes.len(),
                                    "disconnecting slow downstream client due to writer queue saturation"
                                );
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        state.metrics.downstream_disconnect_lagged_total.fetch_add(1, Ordering::Relaxed);
                        disconnect_reason = "broadcast_lagged";
                        warn!(client_id, peer = %peer, "downstream client lagged broadcast channel");
                        break;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        disconnect_reason = "upstream_channel_closed";
                        break;
                    }
                }
            }
        }
    }

    drop(writer_tx);
    let _ = writer_task.await;

    let mut clients = lock_unpoisoned(&state.clients);
    clients.remove(&client_id);
    state
        .metrics
        .downstream_active_clients
        .fetch_sub(1, Ordering::Relaxed);

    info!(
        client_id,
        peer = %peer,
        user = %email,
        read_poll_timeouts,
        reason = disconnect_reason,
        "downstream client disconnected"
    );

    Ok(())
}

/// Writes queued chunks to a downstream client socket.
///
/// Consumes from the writer queue and sends data to the client.
/// Updates forwarded bytes metrics on success.
async fn run_client_writer(
    state: Arc<QbtRelayState>,
    mut writer: tokio::net::tcp::OwnedWriteHalf,
    mut rx: mpsc::Receiver<QueuedChunk>,
) {
    while let Some(item) = rx.recv().await {
        let len = item.bytes.len() as u64;
        if let Err(err) = writer.write_all(&item.bytes).await {
            state
                .metrics
                .bytes_dropped_total
                .fetch_add(len, Ordering::Relaxed);
            debug!(dropped_bytes = len, error = %err, "downstream writer failed");
            break;
        }
        state.add_forwarded(len);
    }
}

/// Monitors forwarding quality and pauses/resumes based on quality ratio.
///
/// Tracks the ratio of forwarded vs attempted bytes over a rolling window.
/// Pauses forwarding when quality drops below threshold, resumes when
/// quality recovers.
async fn run_quality_monitor(
    state: Arc<QbtRelayState>,
    config: QbtRelayConfig,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    while !*shutdown_rx.borrow() {
        tokio::select! {
            _ = shutdown_rx.changed() => return,
            _ = interval.tick() => {
                let ratio = {
                    let mut window = lock_unpoisoned(&state.quality_window);
                    window.rotate();
                    window.ratio()
                };
                let milli = (ratio * 1000.0).round() as u64;
                state.metrics.rolling_quality_milli.store(milli, Ordering::Relaxed);

                let currently_paused = state.metrics.forwarding_paused.load(Ordering::Relaxed);
                if !currently_paused && ratio < config.quality_pause_threshold {
                    state.metrics.forwarding_paused.store(true, Ordering::Relaxed);
                    state.metrics.forwarding_pause_events_total.fetch_add(1, Ordering::Relaxed);
                    warn!(quality_ratio = ratio, pause_threshold = config.quality_pause_threshold, "forwarding paused due to low receive quality");
                } else if currently_paused && ratio >= QUALITY_RESUME_THRESHOLD {
                    state.metrics.forwarding_paused.store(false, Ordering::Relaxed);
                    info!(quality_ratio = ratio, resume_threshold = QUALITY_RESUME_THRESHOLD, "forwarding resumed after quality recovery");
                }
            }
        }
    }
}

/// Returns the current Unix time in seconds.
fn unix_time_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
