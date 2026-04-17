//! End-to-end relay contract tests covering upstream and downstream behavior.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use emwin_protocol::qbt_receiver::{
    QbtRelayConfig, QbtRelayResult, QbtRelayState, build_logon_message, build_server_list_wire,
    run_qbt_relay, xor_ff,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch};

const IO_TIMEOUT: Duration = Duration::from_secs(1);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

struct UpstreamHarness {
    addr: SocketAddr,
    send_tx: mpsc::UnboundedSender<Vec<u8>>,
    ready_rx: Option<oneshot::Receiver<()>>,
}

impl UpstreamHarness {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upstream listener");
        let addr = listener.local_addr().expect("upstream local addr");
        let (send_tx, mut send_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (ready_tx, ready_rx) = oneshot::channel();

        tokio::spawn(async move {
            let accept = tokio::time::timeout(IO_TIMEOUT, listener.accept()).await;
            let Ok(Ok((mut stream, _peer))) = accept else {
                return;
            };
            let _ = ready_tx.send(());

            let mut read_buf = [0_u8; 2048];
            loop {
                tokio::select! {
                    read = stream.read(&mut read_buf) => {
                        match read {
                            Ok(0) | Err(_) => return,
                            Ok(_) => {}
                        }
                    }
                    maybe_payload = send_rx.recv() => {
                        match maybe_payload {
                            Some(payload) => {
                                if stream.write_all(&payload).await.is_err() {
                                    return;
                                }
                            }
                            None => return,
                        }
                    }
                }
            }
        });

        Self {
            addr,
            send_tx,
            ready_rx: Some(ready_rx),
        }
    }

    async fn wait_ready(&mut self) {
        if let Some(ready_rx) = self.ready_rx.take() {
            let _ = tokio::time::timeout(IO_TIMEOUT, ready_rx).await;
        }
    }
}

struct RelayHarness {
    addr: SocketAddr,
    state: Arc<QbtRelayState>,
    shutdown_tx: watch::Sender<bool>,
    join: tokio::task::JoinHandle<QbtRelayResult<()>>,
}

impl RelayHarness {
    async fn start(
        upstream: SocketAddr,
        max_clients: usize,
        auth_timeout: Duration,
        client_buffer_bytes: usize,
    ) -> Self {
        let relay_addr = free_addr().await;
        let config = QbtRelayConfig {
            email: "relay@example.com".to_string(),
            upstream_servers: vec![("127.0.0.1".to_string(), upstream.port())],
            bind_addr: relay_addr,
            max_clients,
            auth_timeout,
            client_buffer_bytes,
            reconnect_delay: Duration::from_millis(100),
            connect_timeout: Duration::from_millis(100),
            quality_window_secs: 60,
            quality_pause_threshold: 0.95,
            metrics_log_interval: Duration::from_secs(30),
        };
        let state = Arc::new(QbtRelayState::from_upstream_servers(
            &config.upstream_servers,
            config.quality_window_secs,
        ));
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let runtime_state = Arc::clone(&state);
        let join =
            tokio::spawn(async move { run_qbt_relay(config, runtime_state, shutdown_rx).await });

        wait_for_port(relay_addr, IO_TIMEOUT).await;
        let readiness_probe = TcpStream::connect(relay_addr)
            .await
            .expect("connect readiness probe");
        wait_for_active_clients(Arc::clone(&state), 1, IO_TIMEOUT).await;
        drop(readiness_probe);
        wait_for_active_clients(Arc::clone(&state), 0, IO_TIMEOUT).await;

        Self {
            addr: relay_addr,
            state,
            shutdown_tx,
            join,
        }
    }

    async fn stop(self) {
        let _ = self.shutdown_tx.send(true);
        let joined = self.join.await.expect("relay task join should succeed");
        joined.expect("relay runtime should stop cleanly");
    }
}

#[tokio::test]
async fn disconnects_client_without_periodic_reauth() {
    let mut upstream = UpstreamHarness::start().await;
    let relay = RelayHarness::start(upstream.addr, 10, Duration::from_millis(500), 65_536).await;
    upstream.wait_ready().await;

    let mut client = TcpStream::connect(relay.addr)
        .await
        .expect("connect relay client");
    send_auth(&mut client, "downstream@example.com").await;

    let mut buf = [0_u8; 1];
    let read = tokio::time::timeout(Duration::from_secs(2), client.read(&mut buf)).await;
    let n = read.expect("read timeout").expect("read failed");
    assert_eq!(n, 0, "client should be disconnected after reauth timeout");

    relay.stop().await;
}

#[tokio::test]
async fn over_capacity_client_gets_server_list_then_disconnects() {
    let mut upstream = UpstreamHarness::start().await;
    let relay = RelayHarness::start(upstream.addr, 0, Duration::from_secs(1), 65_536).await;
    upstream.wait_ready().await;

    let mut first = TcpStream::connect(relay.addr)
        .await
        .expect("connect first client");
    send_auth(&mut first, "first@example.com").await;
    wait_for_active_clients(Arc::clone(&relay.state), 1, IO_TIMEOUT).await;

    let expected_server_list =
        build_server_list_wire(&[("127.0.0.1".to_string(), upstream.addr.port())]);
    let mut second = TcpStream::connect(relay.addr)
        .await
        .expect("connect over-capacity client");
    let mut buf = vec![0_u8; 512];
    let n = tokio::time::timeout(IO_TIMEOUT, second.read(&mut buf))
        .await
        .expect("second client read timeout")
        .expect("second client read failed");
    assert!(n > 0, "second client should receive server list frame");
    assert_eq!(&buf[..n], &expected_server_list[..]);

    let n2 = tokio::time::timeout(IO_TIMEOUT, second.read(&mut buf))
        .await
        .expect("second disconnect read timeout")
        .expect("second disconnect read failed");
    assert_eq!(n2, 0, "second client should disconnect after server list");

    drop(first);

    relay.stop().await;
}

#[tokio::test]
async fn slow_client_is_disconnected_when_buffer_budget_is_exceeded() {
    let mut upstream = UpstreamHarness::start().await;
    let send_tx = upstream.send_tx.clone();
    let relay = RelayHarness::start(upstream.addr, 10, Duration::from_secs(1), 32).await;
    upstream.wait_ready().await;

    let mut client = TcpStream::connect(relay.addr)
        .await
        .expect("connect slow client");
    send_auth(&mut client, "slow@example.com").await;

    for _ in 0..200 {
        send_tx
            .send(vec![0xAA; 128])
            .expect("send upstream payload to relay");
    }

    let mut buf = [0_u8; 1];
    let n = tokio::time::timeout(IO_TIMEOUT, client.read(&mut buf))
        .await
        .expect("slow client disconnect timeout")
        .expect("slow client read failed");
    assert_eq!(n, 0, "slow client should be disconnected on queue overflow");

    relay.stop().await;
}

#[tokio::test]
async fn health_snapshot_reports_status_and_active_clients() {
    let mut upstream = UpstreamHarness::start().await;
    let relay = RelayHarness::start(upstream.addr, 10, Duration::from_secs(1), 65_536).await;
    upstream.wait_ready().await;

    let initial = relay.state.health_snapshot();
    assert_eq!(initial.status, "ok");
    assert!(!initial.forwarding_paused);
    assert_eq!(initial.downstream_active_clients, 0);

    let mut client = TcpStream::connect(relay.addr)
        .await
        .expect("connect client for health test");
    send_auth(&mut client, "health@example.com").await;

    wait_for_active_clients(Arc::clone(&relay.state), 1, IO_TIMEOUT).await;
    let while_connected = relay.state.health_snapshot();
    assert_eq!(while_connected.status, "ok");
    assert_eq!(while_connected.downstream_active_clients, 1);

    drop(client);
    wait_for_active_clients(Arc::clone(&relay.state), 0, IO_TIMEOUT).await;
    let after_disconnect = relay.state.health_snapshot();
    assert_eq!(after_disconnect.status, "ok");
    assert_eq!(after_disconnect.downstream_active_clients, 0);

    relay.stop().await;
}

#[tokio::test]
async fn health_snapshot_reports_forwarding_paused_when_quality_drops() {
    let mut upstream = UpstreamHarness::start().await;
    let send_tx = upstream.send_tx.clone();
    let relay = RelayHarness::start(upstream.addr, 10, Duration::from_secs(1), 16).await;
    upstream.wait_ready().await;

    let mut client = TcpStream::connect(relay.addr)
        .await
        .expect("connect client for paused health test");
    send_auth(&mut client, "paused@example.com").await;

    for _ in 0..200 {
        send_tx
            .send(vec![0xAB; 128])
            .expect("send upstream payload to relay");
    }

    wait_for_forwarding_paused(Arc::clone(&relay.state), true, IO_TIMEOUT).await;
    let paused = relay.state.health_snapshot();
    assert_eq!(paused.status, "ok");
    assert!(paused.forwarding_paused);

    relay.stop().await;
}

async fn free_addr() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral addr");
    listener.local_addr().expect("ephemeral local addr")
}

async fn wait_for_port(addr: SocketAddr, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if TcpStream::connect(addr).await.is_ok() {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {addr}");
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn send_auth(stream: &mut TcpStream, email: &str) {
    let logon = build_logon_message(email);
    let wire = xor_ff(logon.as_bytes());
    stream.write_all(&wire).await.expect("send downstream auth");
}

async fn wait_for_active_clients(state: Arc<QbtRelayState>, expected: u64, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let current = state.health_snapshot().downstream_active_clients;
        if current == expected {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for active clients {expected}, got {current}"
        );
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

async fn wait_for_forwarding_paused(state: Arc<QbtRelayState>, expected: bool, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let current = state.health_snapshot().forwarding_paused;
        if current == expected {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for forwarding_paused={expected}, got {current}"
        );
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}
