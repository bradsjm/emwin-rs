//! Integration tests for reconnect and failover behavior in the QBT client runtime.

use crate::support::{build_frame, build_header};
use emwin_protocol::qbt_receiver::{
    QbtDecodeConfig, QbtReceiver, QbtReceiverClient, QbtReceiverConfig, QbtReceiverError,
    QbtReceiverEvent, calculate_qbt_checksum,
};
use futures::{FutureExt, StreamExt};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::time::{Duration, advance};

fn encoded_valid_data_frame() -> Vec<u8> {
    let body = [b'R'; 1024];
    let checksum = calculate_qbt_checksum(&body) as u32;
    let header = build_header("reconnect.bin", 1, 1, checksum, None);
    build_frame(header, &body)
}

#[tokio::test(start_paused = true)]
async fn watchdog_timeout_reconnects_without_termination() {
    let listener_a = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let address_a = listener_a
        .local_addr()
        .expect("listener should have local addr");

    let listener_b = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let address_b = listener_b
        .local_addr()
        .expect("listener should have local addr");

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let accepted_connections = Arc::new(AtomicUsize::new(0));

    let spawn_server = |listener: TcpListener,
                        mut local_shutdown_rx: watch::Receiver<bool>,
                        accepted_connections_task: Arc<AtomicUsize>| {
        tokio::spawn(async move {
            let payload = encoded_valid_data_frame();
            loop {
                tokio::select! {
                    changed = local_shutdown_rx.changed() => {
                        if changed.is_ok() && *local_shutdown_rx.borrow() {
                            break;
                        }
                    }
                    accepted = listener.accept() => {
                        let Ok((mut socket, _)) = accepted else {
                            break;
                        };
                        accepted_connections_task.fetch_add(1, Ordering::Relaxed);

                        let mut auth_buf = [0u8; 128];
                        let _ = tokio::time::timeout(Duration::from_millis(200), socket.read(&mut auth_buf)).await;

                        if socket.write_all(&payload).await.is_err() {
                            continue;
                        }

                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
            }
        })
    };

    let server_task_a = spawn_server(
        listener_a,
        shutdown_rx.clone(),
        Arc::clone(&accepted_connections),
    );
    let server_task_b = spawn_server(
        listener_b,
        shutdown_rx.clone(),
        Arc::clone(&accepted_connections),
    );

    let mut client = QbtReceiver::builder(QbtReceiverConfig {
        email: "test@example.com".to_string(),
        servers: vec![
            ("127.0.0.1".to_string(), address_a.port()),
            ("127.0.0.1".to_string(), address_b.port()),
        ],
        server_list_path: None,
        follow_server_list_updates: true,
        reconnect_delay_secs: 1,
        connection_timeout_secs: 1,
        write_timeout_secs: 1,
        watchdog_timeout_secs: 1,
        max_exceptions: 10,
        decode: QbtDecodeConfig::default(),
    })
    .build()
    .expect("client should build");

    client.start().expect("client should start");
    let mut events = client.events().expect("events should be available");

    let mut connected_events = 0u32;
    let mut watchdog_timeout_errors = 0u32;

    for _ in 0..16 {
        tokio::task::yield_now().await;
        drain_receiver_events(
            &mut events,
            &mut connected_events,
            &mut watchdog_timeout_errors,
        );
        if connected_events >= 2 && watchdog_timeout_errors >= 1 {
            break;
        }
        advance(Duration::from_secs(1)).await;
    }

    shutdown_tx
        .send(true)
        .expect("server shutdown signal should send");
    server_task_a.await.expect("server task a should join");
    server_task_b.await.expect("server task b should join");
    drop(events);
    client.stop().await.expect("client should stop");

    assert!(
        connected_events >= 2,
        "expected reconnect after watchdog timeout"
    );
    assert!(
        watchdog_timeout_errors >= 1,
        "expected watchdog timeout to be surfaced"
    );
    assert!(
        accepted_connections.load(Ordering::Relaxed) >= 1,
        "expected server to observe at least one accepted connection"
    );
}

#[tokio::test(start_paused = true)]
async fn failed_endpoint_rotates_to_next_server_without_waiting_for_full_delay() {
    let failed_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed listener should bind");
    let failed_port = failed_listener
        .local_addr()
        .expect("failed listener should have local addr")
        .port();
    drop(failed_listener);

    let listener_ok = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let address_ok = listener_ok
        .local_addr()
        .expect("listener should have local addr");

    let server_task = tokio::spawn(async move {
        let payload = encoded_valid_data_frame();
        let (mut socket, _) = listener_ok.accept().await.expect("client should connect");
        let mut auth_buf = [0u8; 128];
        let _ = tokio::time::timeout(Duration::from_millis(200), socket.read(&mut auth_buf)).await;
        socket
            .write_all(&payload)
            .await
            .expect("payload write should succeed");
    });

    let mut client = QbtReceiver::builder(QbtReceiverConfig {
        email: "test@example.com".to_string(),
        servers: vec![
            ("127.0.0.1".to_string(), failed_port),
            ("127.0.0.1".to_string(), address_ok.port()),
        ],
        server_list_path: None,
        follow_server_list_updates: false,
        reconnect_delay_secs: 5,
        connection_timeout_secs: 1,
        write_timeout_secs: 1,
        watchdog_timeout_secs: 5,
        max_exceptions: 10,
        decode: QbtDecodeConfig::default(),
    })
    .build()
    .expect("client should build");

    client.start().expect("client should start");
    let mut events = client.events().expect("events should be available");
    let mut connected_endpoint = None;

    for _ in 0..30 {
        tokio::task::yield_now().await;
        if let Some(endpoint) = drain_connected_endpoint(&mut events) {
            connected_endpoint = Some(endpoint);
            break;
        }
        advance(Duration::from_millis(100)).await;
    }

    drop(events);
    client.stop().await.expect("client should stop");
    server_task.await.expect("server task should join");
    let expected = format!("127.0.0.1:{}", address_ok.port());

    assert_eq!(connected_endpoint.as_deref(), Some(expected.as_str()),);
}

fn drain_receiver_events(
    events: &mut (impl futures::Stream<Item = Result<QbtReceiverEvent, QbtReceiverError>> + Unpin),
    connected_events: &mut u32,
    watchdog_timeout_errors: &mut u32,
) {
    loop {
        match events.next().now_or_never() {
            Some(Some(Ok(QbtReceiverEvent::Connected(_)))) => {
                *connected_events = connected_events.saturating_add(1);
            }
            Some(Some(Err(QbtReceiverError::Lifecycle(message))))
                if message == "watchdog timeout" =>
            {
                *watchdog_timeout_errors = watchdog_timeout_errors.saturating_add(1);
            }
            Some(Some(_)) => {}
            Some(None) | None => break,
        }
    }
}

fn drain_connected_endpoint(
    events: &mut (impl futures::Stream<Item = Result<QbtReceiverEvent, QbtReceiverError>> + Unpin),
) -> Option<String> {
    loop {
        match events.next().now_or_never() {
            Some(Some(Ok(QbtReceiverEvent::Connected(endpoint)))) => {
                return Some(endpoint);
            }
            Some(Some(_)) => {}
            Some(None) | None => return None,
        }
    }
}
