# emwin-protocol

Core Rust library for EMWIN protocol parsing, client runtime, and file assembly.

## What it provides

- **QBT Receiver** (`qbt_receiver`): EMWIN QBT TCP receiver client
  - Protocol layer
    - XOR wire transform handling (`0xFF`)
    - Stateful frame decoder (`/PF`, `/ServerList`)
    - V1 + V2 body handling and checksum validation policies
    - Compression policy for V2 payloads (`RequireZlibHeader`, `TryAlways`)
  - Client runtime
    - TCP connection loop with round-robin endpoint rotation
    - Authentication heartbeat
    - Watchdog health monitoring
    - Event stream + handler fanout with fault isolation
    - Server-list persistence and rotation management
  - File layer
    - Segment-to-file assembly
    - Duplicate block/file suppression

- **Weather Wire Receiver** (`wxwire_receiver`): NWWS-OI XMPP-based receiver
  - Custom XMPP transport with STARTTLS and SASL authentication
  - XEP-0198 stream management for reliability
  - MUC room joining for product broadcasts
  - Full-file event emission (not segmented)

- **Unified Ingest** (`ingest`): Single abstraction over QBT and Weather Wire
  - Normalized product events regardless of source
  - Common telemetry and warning handling

## Features

This crate uses Cargo features to enable receiver implementations:

- `qbt`: QBT TCP receiver
- `wxwire`: Weather Wire receiver
- `telemetry-serde`: Enable serialization for telemetry snapshots

No transport feature is enabled by default. Downstream crates must explicitly select `qbt`, `wxwire`, or both.

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
emwin-protocol = { git = "https://github.com/bradsjm/emwin-rs", default-features = false, features = ["qbt"] }
```

## Examples

### QBT Receiver

```rust
use emwin_protocol::qbt_receiver::{
    QbtDecodeConfig, QbtReceiver, QbtReceiverClient, QbtReceiverConfig,
    default_qbt_upstream_servers,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = QbtReceiverConfig {
        email: "you@example.com".to_string(),
        servers: default_qbt_upstream_servers(),
        server_list_path: None,
        follow_server_list_updates: true,
        reconnect_delay_secs: 5,
        connection_timeout_secs: 5,
        write_timeout_secs: 10,
        watchdog_timeout_secs: 49,
        max_exceptions: 10,
        decode: QbtDecodeConfig::default(),
    };
    
    let mut receiver = QbtReceiver::builder(config).build()?;
    receiver.start()?;
    
    // Receive events
    let mut events = receiver.events()?;
    while let Some(event) = events.next().await {
        println!("{:?}", event?);
    }
    
    receiver.stop().await?;
    Ok(())
}
```

### Weather Wire Receiver

```rust
use emwin_protocol::wxwire_receiver::{
    WxWireReceiver, WxWireReceiverClient, WxWireReceiverConfig,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = WxWireReceiverConfig {
        username: "you@example.com".to_string(),
        password: "secret".to_string(),
        write_timeout_secs: 10,
        ..WxWireReceiverConfig::default()
    };
    
    let mut receiver = WxWireReceiver::builder(config).build()?;
    receiver.start()?;
    
    // Receive events
    let mut events = receiver.events()?;
    while let Some(event) = events.next().await {
        println!("{:?}", event?);
    }
    
    receiver.stop().await?;
    Ok(())
}
```

### Unified Ingest Receiver

```rust
use emwin_protocol::ingest::{IngestConfig, IngestEvent, IngestReceiver};
use emwin_protocol::qbt_receiver::{QbtDecodeConfig, QbtReceiverConfig, default_qbt_upstream_servers};
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = QbtReceiverConfig {
        email: "you@example.com".to_string(),
        servers: default_qbt_upstream_servers(),
        server_list_path: None,
        follow_server_list_updates: true,
        reconnect_delay_secs: 5,
        connection_timeout_secs: 5,
        write_timeout_secs: 10,
        watchdog_timeout_secs: 49,
        max_exceptions: 10,
        decode: QbtDecodeConfig::default(),
    };
    
    let mut receiver = IngestReceiver::build(IngestConfig::Qbt(config))?;
    receiver.start()?;
    
    let mut ingest_stream = receiver.events()?;
    
    while let Some(event) = ingest_stream.next().await {
        match event? {
            IngestEvent::Product(product) => {
                println!("Received product: {}", product.filename);
            }
            IngestEvent::Connected { endpoint } => {
                println!("Connected to: {}", endpoint);
            }
            IngestEvent::Disconnected => {
                println!("Disconnected");
            }
            IngestEvent::Telemetry(telemetry) => {
                println!("Telemetry: {:?}", telemetry);
            }
            IngestEvent::Warning(warning) => {
                eprintln!("Warning: {:?}", warning);
            }
        }
    }
    
    receiver.stop().await?;
    Ok(())
}
```

### Protocol Decoder (Low-level)

```rust
use emwin_protocol::qbt_receiver::protocol::codec::{QbtFrameDecoder, QbtProtocolDecoder};

fn decode_wire_chunk(wire: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = QbtProtocolDecoder::default();
    let events = decoder.feed(wire)?;
    println!("Decoded {} event(s)", events.len());
    for event in events {
        println!("{:?}", event);
    }
    Ok(())
}
```

## Public Modules

- `ingest`: Unified product ingestion abstraction
- `qbt_receiver`: EMWIN QBT TCP receiver
- `wxwire_receiver`: Weather Wire XMPP receiver

## Key Exported Types

### QBT Receiver

- `QbtReceiver`, `QbtReceiverBuilder`, `QbtReceiverClient`, `QbtReceiverEvent`
- `QbtReceiverConfig`, `QbtDecodeConfig`, `QbtChecksumPolicy`, `QbtV2CompressionPolicy`
- `QbtFrameDecoder`, `QbtProtocolDecoder`, `QbtFrameEncoder`
- `QbtFrameEvent`, `QbtSegment`, `QbtServerList`
- `QbtFileAssembler`, `QbtCompletedFile`
- `QbtReceiverTelemetrySnapshot`

### Weather Wire Receiver

- `WxWireReceiver`, `WxWireReceiverBuilder`, `WxWireReceiverClient`, `WxWireReceiverEvent`
- `WxWireReceiverConfig`
- `WxWireDecoder`, `WxWireFrameDecoder`
- `WxWireReceiverFile`, `WxWireReceiverFrameEvent`, `WxWireReceiverWarning`
- `WxWireReceiverTelemetrySnapshot`

### Unified Ingest

- `IngestReceiver`, `IngestConfig`
- `IngestEvent`, `ReceivedProduct`, `ProductOrigin`
- `IngestTelemetry`, `IngestWarning`, `IngestError`

## Receiver Lifecycle Contract

- Receiver configs are validated before startup via `build()`.
- `start()` may only be called once per running instance.
- `events()` is a single-consumer subscription; calling it more than once returns an error.
- `stop()` is idempotent and aborts the background task if shutdown exceeds the timeout window.

## Configuration

### QBT Receiver Configuration

```rust
QbtReceiverConfig {
    email: "you@example.com".to_string(),          // Authentication email
    servers: default_qbt_upstream_servers(),       // Initial EMWIN server list
    server_list_path: None,                        // Optional persistence path for automatic mode
    follow_server_list_updates: true,              // Accept upstream server-list replacements
    reconnect_delay_secs: 5,                       // Delay after one full failed server pass
    connection_timeout_secs: 5,                    // TCP connect timeout
    write_timeout_secs: 10,                        // Socket write timeout
    watchdog_timeout_secs: 49,                     // Health check timeout
    max_exceptions: 10,                            // Max exceptions before reconnect
    decode: QbtDecodeConfig::default(),            // Decoder policies and server-list buffer cap
}
```

### Weather Wire Receiver Configuration

```rust
WxWireReceiverConfig {
    username: "you@example.com".to_string(),       // NWWS-OI username
    password: "secret".to_string(),                // NWWS-OI password
    idle_timeout_secs: 90,                         // Idle warning threshold
    event_channel_capacity: 1024,                  // Event buffer size
    inbound_channel_capacity: 512,                 // Stanza buffer size
    telemetry_emit_interval_secs: 5,               // Telemetry frequency
    connect_timeout_secs: 10,                      // Connection timeout
    write_timeout_secs: 10,                        // Socket write timeout
}
```

## Quality Gates

From workspace root:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p emwin-protocol
cargo test -p emwin-protocol --features qbt
cargo test -p emwin-protocol --features qbt --test qbt_receiver
```

## Protocol Documentation

See the `docs/` directory for detailed protocol specifications:

- `docs/EMWIN QBT TCP Protocol.md`: QBT protocol specification
- `docs/weather-wire.md`: Weather Wire protocol specification

## License

This project is licensed under the MIT License - see the LICENSE file for details.
