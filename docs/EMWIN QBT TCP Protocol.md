# EMWIN TCP Protocol Reference Specification

Version: 2.0.0
Last Updated: 2026-03-03
Status: Authoritative (implementation and tests)

## 1. Purpose

This is the authoritative, evidence-based TCP protocol reference for `emwin-rs`.

- It supersedes `docs/EMWIN QBT Satellite Broadcast Protocol draft v1.0.3.md` for implementation behavior.
- It is both normative (what code must do) and explanatory (why it does it).
- It covers the full stack from wire bytes to final decoded message and file reconstruction.

## 2. Relationship to the EMWIN Satellite Draft

EMWIN is protocol-compatible with EMWIN QBT payload semantics where practical, but it runs over TCP rather than satellite broadcast framing.

Use this rule set:

1. `docs/EMWIN QBT Satellite Broadcast Protocol draft v1.0.3.md` is a historical/reference input.
2. This document is the implementation authority for EMWIN/QBT TCP behavior.
3. `docs/weather-wire.md` is the implementation authority for Weather Wire/XMPP behavior.
4. When draft assumptions conflict with observed TCP behavior, this document wins.

High-level mapping:

| Satellite draft concept | EMWIN TCP interpretation |
|---|---|
| Fixed-size packet transmission model | Continuous TCP stream with arbitrary segmentation |
| Packet boundaries implied by transport | Boundaries inferred by decoded sync marker and parsed lengths |
| Suffix strictness in packet diagrams | Suffix tolerated but not required for decode continuity |
| Draft checksum interpretation examples | Receiver validates with low-16-bit compatibility semantics |

The payload/header field lineage remains EMWIN-derived (`/PF`, `/PN`, `/PT`, `/CS`, `/FD`, `/DL`), but transport behavior is TCP-native.

### 2.1 Satellite-to-TCP Glossary

| Satellite draft term | EMWIN TCP term | Practical meaning |
|---|---|---|
| Packet | Frame candidate in byte stream | A decodable unit inferred from sync + parsed lengths, not from socket read size |
| Broadcast packet order | Stream decode order | Receiver processes bytes in arrival order from one TCP stream |
| Prefix (6 null) | Sync marker | Six decoded `0x00` bytes used for resynchronization |
| Header (80 bytes) | Header (80 bytes) | Same payload header concept, parsed after sync |
| Data block | Segment content | V1 fixed 1024 bytes, V2 variable length from `/DL` |
| Suffix (6 null) | Optional trailing delimiter | Commonly present but not required for decode continuity |
| Computed sum (`/CS`) | Receiver checksum compare target | Receiver validates with low-16-bit compatibility semantics |
| Product retransmission assumptions | Runtime recovery model | Reliability is achieved by decode recovery + reconnect behavior over TCP |

## 3. One-Page Mental Model

```text
TCP stream bytes (wire, XOR-FF encoded)
  -> XOR-FF decode
  -> sync scan (decoded 00 00 00 00 00 00)
  -> frame typing (/PF data frame, /ServerList frame)
  -> header parse + body extraction (V1 fixed 1024, V2 /DL)
  -> optional V2 zlib inflate
  -> checksum validation + policy filters
  -> FrameEvent stream (DataBlock | ServerListUpdate | Warning)
  -> optional FileAssembler reconstruction
  -> completed file payload(s)
```

## 4. Layered Architecture (Wire to Final Message)

```text
L0  Transport        : TCP byte stream (segmentation arbitrary)
L1  Wire Transform   : XOR-FF decode/encode
L2  Framing          : sync detection + frame classification
L3  Payload Decode   : header parse, body extraction, optional inflate
L4  Validation       : checksum, structural guards, policy drops
L5  Event Projection : FrameEvent::{DataBlock, ServerListUpdate, Warning}
L6  Reconstruction   : FileAssembler combines QbtSegment blocks
```

Implementation anchors:

- L1-L5: `crates/emwin-protocol/src/protocol/codec.rs`
- checksum: `crates/emwin-protocol/src/protocol/checksum.rs`
- compression: `crates/emwin-protocol/src/protocol/compression.rs`
- server list parsing: `crates/emwin-protocol/src/protocol/server_list.rs`
- runtime dispatch/recovery: `crates/emwin-protocol/src/client/mod.rs`
- L6 assembly: `crates/emwin-protocol/src/file/assembler.rs`

## 5. Wire-Level Specification (TCP)

### 5.1 Transport and Encoding

1. The protocol runs over a TCP byte stream.
2. Every inbound payload byte is decoded via XOR with `0xFF` before any parse logic.
3. Outbound auth/logon payload is also XOR-FF encoded.
4. Read boundaries from `recv/read` calls are not frame boundaries.

Auth message format (pre-XOR):

```text
ByteBlast Client|NM-<email>|V2
```

### 5.2 Sync and Frame Boundaries

Decoded sync marker:

```text
00 00 00 00 00 00
```

Wire equivalent (before XOR decode):

```text
FF FF FF FF FF FF
```

Boundary rules:

- Decoder searches sliding windows for decoded six-null sync.
- TCP packet boundaries are irrelevant; frames may span reads.
- On failure to find sync, decoder retains trailing 5 bytes as overlap window.

### 5.3 Frame Type Identification

After sync/padding removal:

- `/PF...` => data block frame
- `/Se...` or `/ServerList/...` => server list frame
- unknown prefix => byte-skip + resync attempt

## 6. Data Frame Format (Decoded Stream)

### 6.1 Layout Diagram

```text
Offset (decoded stream)

0      5   6                    85  86                       N   N+5
|------|---|---------------------|---|------------------------|----|
 sync6     header (80 bytes)         body (V1=1024, V2=/DL)   optional suffix6

sync6            : 00 00 00 00 00 00
header           : ASCII, must satisfy the fixed-width decoder grammar
body             : raw payload bytes (may be zlib-compressed for V2)
optional suffix6 : often 00x6 on wire; not required for acceptance
```

### 6.2 Header Grammar

Regex-equivalent grammar implemented by decoder:

```text
/PF<filename>
 /PN <u32>
 /PT <u32>
 /CS <u32>
 /FD<timestamp>
 [ /DL<u32> ]
\r\n
[optional trailing spaces to fill fixed 80 bytes]
```

Timestamp parse format:

```text
[month]/[day]/[year] [hour(12)]:[minute]:[second] [AM|PM]
```

### 6.3 V1 and V2 Body Rules

- V1: `/DL` absent, body length fixed at 1024.
- V2: `/DL` present, body length = `/DL`, bounded by `1..=max_v2_body_size`.
- default `max_v2_body_size = 1024`.

### 6.4 Compression Rules (V2)

Default policy `RequireZlibHeader`:

- attempt inflate only if body starts with one of:
  - `78 9C`
  - `78 DA`
  - `78 01`

If inflate fails:

- emit `ProtocolWarning::DecompressionFailed`
- drop that segment
- continue stream decode

### 6.5 Checksum Rules

Receiver checksum function:

```text
calculate_checksum(data) = sum(unsigned bytes) & 0xFFFF
```

Comparison behavior:

- V1 expected checksum masked to low 16 bits before compare.
- V2 expected checksum compared through same `verify_checksum` path.
- mismatch emits `ProtocolWarning::ChecksumMismatch` and segment is dropped.

Important wire compatibility note:

- live evidence shows V1 headers can carry full-sum values beyond 16 bits,
  while receiver remains low-16 compatible.

### 6.6 Text Payload Normalization

For filenames ending `.TXT` or `.WMO` (case-insensitive):

- trailing bytes are trimmed if they are one of:
  - `\0`, space, tab, CR, LF

### 6.7 Non-Emitted Data Cases

Decoder drops (no `DataBlock` event) when any of these hold:

- decompression failed
- invalid block numbering (`total=0`, `block=0`, `block>total`)
- filename `FILLFILE.TXT` (case-insensitive)
- checksum mismatch

## 7. Server List Frame Format

### 7.1 Supported Forms

Simple form:

```text
/ServerList/<host:port|host:port|...>\0
```

Full form:

```text
/ServerList/<regular servers>\ServerList\/SatServers/<sat servers>\SatServers\\0
```

Delimiter semantics:

- regular servers: `|`
- any trailing `SatServers` section is ignored by the receiver/client model

Malformed entries:

- are filtered
- emit `ProtocolWarning::MalformedServerEntry`
- do not invalidate valid entries in same frame

### 7.2 TCP Operational Use of Server Lists

For EMWIN TCP operation, only the regular `/ServerList/...` entries are used as candidate
connection endpoints.

Operational behavior:

- Server list frames are transmitted regularly by upstream and treated as live endpoint state.
- Each received server list update replaces the runtime candidate list after validation/filtering.
- The replacement list is sorted, deduplicated, and used for deterministic round-robin rotation.
- The client attempts each candidate endpoint at most once per pass.
- On connection loss/failure, the client immediately hops to the next endpoint in the pass.
- After one full pass fails to establish a surviving connection, the client waits
  `reconnect_delay_secs` before starting the next pass.
- Connection attempts are short-lived and clamped to a maximum of 5 seconds per endpoint.
- When the caller pins servers explicitly, automatic server-list load/save/update behavior is disabled.
- Relay passthrough preserves raw upstream `/ServerList/...` frames as received; relay forwarding
  does not normalize or rewrite those wire bytes.

Why this matters:

- Internet-connected TCP endpoints have higher outage probability than one-way satellite reception.
- Rapid endpoint hopping reduces data loss windows during outages.
- Priority files are retransmitted (see Section 12), so short failover gaps can be recovered by the
  next transmission cycle.

## 8. Decoder State Machine

```text
Resync -> StartFrame -> FrameType -> (ServerList | BlockHeader)
BlockHeader -> BlockBody -> Validate -> StartFrame
ServerList -> StartFrame

Any recoverable parse/decode error:
  emit Warning(DecoderRecovered)
  clear pending segment
  reset to Resync
```

Recovery strategy is forward-only; process does not terminate on recoverable corruption.

## 9. Final Decoded Message Model

Primary decoded data unit is `QbtSegment`:

```text
filename: String
block_number: u32
total_blocks: u32
content: Bytes
checksum: u32
length: usize
version: ProtocolVersion (V1|V2)
timestamp_utc: SystemTime
source: Option<SocketAddr>
```

Event envelope type is `FrameEvent`:

- `DataBlock(QbtSegment)`
- `ServerListUpdate(ServerList)`
- `Warning(ProtocolWarning)`

Client runtime envelope type is `ClientEvent`:

- `Frame(FrameEvent)`
- `Connected(String)`
- `Disconnected`
- `Telemetry(ClientTelemetrySnapshot)`

Warning types:

- `ChecksumMismatch`
- `DecompressionFailed`
- `DecoderRecovered`
- `MalformedServerEntry`
- `TimestampParseFallback`
- `HandlerError`
- `BackpressureDrop`

## 10. Runtime and Dispatch Flow

```text
socket.read -> decoder.feed
  -> FrameEvents
    -> apply server list updates
    -> invoke subscribed handlers (isolated)
    -> publish ClientEvent::Frame over bounded channel (1024)

channel full:
  -> drop event
  -> increment telemetry counters
  -> attempt emit Warning(BackpressureDrop)
```

Reconnect/survivability behavior:

- watchdog timeout closes current session and triggers reconnect loop
- one-pass round-robin endpoint rotation is applied before each reconnect delay
- process continues unless explicitly stopped

### 10.1 Core Telemetry Schema

`emwin-protocol` maintains process-local runtime counters exposed via
`Client::telemetry_snapshot()`.

Counter fields:

- `connection_attempts_total`
- `connection_success_total`
- `connection_fail_total`
- `disconnect_total`
- `watchdog_timeouts_total`
- `watchdog_exception_events_total`
- `auth_logon_sent_total`
- `bytes_in_total`
- `frame_events_total`
- `data_blocks_emitted_total`
- `server_list_updates_total`
- `checksum_mismatch_total`
- `decompression_failed_total`
- `decoder_recovery_events_total`
- `handler_failures_total`
- `backpressure_warning_emitted_total`
- `event_queue_drop_total`
- `telemetry_events_emitted_total`

These counters support runtime diagnostics and protocol conformance observability.

## 11. File Reconstruction Layer

`FileAssembler` reconstitutes complete files from `QbtSegment` blocks.

### 11.1 Why Blocks Arrive Out of Order or Interleaved

The upstream transmission model is priority-driven. Higher-importance products (for example,
tornado warnings) are allowed to preempt lower-importance, long-running transfers (for example,
large graphics or long forecast text products).

This is intentional behavior and is required to minimize latency for urgent products while still
keeping the channel busy with bulk data.

Implications for receivers:

- Segments for one filename may be interrupted by segments from another filename.
- Segment arrival order is not a global filename order; reconstruction must key by file identity
  and block numbering, not arrival adjacency.
- A file may complete while another older file remains incomplete.

Interleaved reception example:

```text
Arrival index   Filename        Block/Total   Notes
-----------     --------        -----------   -----
1               GRAPHIC1.ZIP    1/40          low-priority bulk transfer starts
2               GRAPHIC1.ZIP    2/40
3               WARN1234.TXT    1/2           high-priority warning preempts
4               WARN1234.TXT    2/2           warning completes quickly
5               GRAPHIC1.ZIP    3/40          bulk transfer resumes
6               GRAPHIC1.ZIP    4/40
...
```

Receiver reconstruction model (implemented by `FileAssembler`) is therefore per-file and
order-aware at block level, not stream-order-dependent.

### 11.2 High-Priority Retransmission and Recovery Window

High-priority files are transmitted twice to improve successful reception probability.

- Retransmission starts no sooner than 5 seconds after first transmission.
- Retransmission may be later than 5 seconds depending on queue state.

Implication for TCP failover:

- If blocks are missed during endpoint switchover, high-priority retransmission provides a
  practical recovery path with minimal end-to-end data loss for urgent products.

### 11.3 Reassembly Algorithm

Algorithm summary:

1. Group segments by key: `<lower(filename)>_<timestamp_utc>`.
2. Insert segment by `block_number` into ordered map.
3. When map size equals `total_blocks`, concatenate in block order.
4. Emit `CompletedFile { filename, data }`.
5. Cache completed keys to suppress duplicate reconstruction.

### 11.4 Incomplete-File Retention Limits

Incomplete file state is bounded to protect memory and force eventual cleanup under sustained loss:

- Inflight TTL: 90 seconds (default).
- Max inflight files: 256 (default).

Behavior:

- Inflight entries not updated within TTL are evicted.
- If inflight set exceeds limit, oldest entries are evicted.
- Completed-file duplicate suppression remains separately bounded by duplicate cache size.

Guardrails:

- ignores `FILLFILE.TXT`
- ignores invalid block numbering

## 12. Worked Examples

### 12.1 Decoded V1 Header Example

```text
/PFZFPSFOCA.TXT /PN 3 /PT 5 /CS 63366 /FD5/19/2016 5:24:26 PM\r\n
[space padded to 80 bytes]
```

Interpretation:

- filename: `ZFPSFOCA.TXT`
- segment: 3 of 5
- checksum field value may exceed 16 bits on wire
- body length: 1024 (no `/DL`)

### 12.2 Decoded V2 Header Example

```text
/PFABC12345.ZIP /PN 1 /PT 1 /CS 32123 /FD3/3/2026 1:02:03 PM /DL314\r\n
[space padded to 80 bytes]
```

Interpretation:

- V2 body length = 314 bytes
- if body starts with zlib signature, decoder attempts inflate

## 13. Requirement-to-Test Matrix

| ID | Normative Requirement | Unit Test Target | Integration Test Target | Property/Fuzz | Status |
|---|---|---|---|---|---|
| P-001 | Inbound transport bytes are XOR-decoded with key `0xFF` before parsing | `protocol::codec::tests::find_sync_recovers_after_garbage` | `crates/emwin-protocol/tests/protocol_parity.rs::sync_recovery` | Yes | Required |
| P-002 | Sync detection uses 6 decoded `0x00` bytes | `protocol::codec::tests::find_sync_recovers_after_garbage` | `crates/emwin-protocol/tests/protocol_parity.rs::sync_recovery` | Yes | Implemented |
| P-003 | V1 header is exactly 80 bytes and satisfies the fixed-width decoder grammar | `protocol::codec::tests::parse_header_valid`, `protocol::codec::tests::parse_header_invalid_missing_fields` | `crates/emwin-protocol/tests/protocol_parity.rs::inspect_valid_v1_fixture` | Yes | Implemented |
| P-004 | V2 `/DL` length is required and bounded `1..=1024` | `protocol::codec::tests::v2_dl_bounds` | `crates/emwin-protocol/tests/protocol_parity.rs::v2_fixture_bounds` | Yes | Implemented |
| P-005 | V2 decompression is header-gated under `RequireZlibHeader` | `protocol::codec::tests::v2_header_gate` | `N/A (unit + property coverage)` | Yes | Implemented |
| P-006 | Decompression failure never aborts runtime; bad segment is dropped | `protocol::codec::tests::v2_decompress_failure_drops_segment_and_emits_warning` | `crates/emwin-protocol/tests/protocol_parity.rs::v2_corrupt_payload_policy` | Yes | Required |
| P-007 | Receiver checksum comparison uses `sum(data) & 0xFFFF` | `protocol::checksum::tests::checksum_matches_reference` | `crates/emwin-protocol/tests/protocol_parity.rs::checksum_fixture` | Yes | Implemented |
| P-008 | V1 expected checksum is masked to 16-bit | `protocol::codec::tests::v1_checksum_masking` | `crates/emwin-protocol/tests/protocol_parity.rs::checksum_fixture` | No | Implemented |
| P-009 | Checksum mismatch payload is never emitted as data event | `protocol::codec::tests::checksum_strict_drop` | `crates/emwin-protocol/tests/protocol_parity.rs::stream_policy_behavior` | Yes | Required |
| P-010 | `FILLFILE.TXT` is never emitted as data event | `protocol::codec::tests::fillfile_filtered` | `N/A (unit coverage)` | No | Implemented |
| P-011 | `.TXT` and `.WMO` trailing padding is trimmed | `protocol::codec::tests::trim_padding_text_wmo` | `N/A (unit coverage)` | No | Implemented |
| P-012 | Simple server list format parses and filters invalid entries | `protocol::server_list::tests::server_list_simple_parse` | `crates/emwin-protocol/tests/protocol_parity.rs::server_update_simple` | Yes | Implemented |
| P-013 | Full server list format preserves regular entries and ignores any satellite suffix | `protocol::server_list::tests::server_list_full_parse_ignores_satellite_entries` | `crates/emwin-protocol/tests/qbt_receiver.rs::full_server_list_frame_ignores_satellite_entries` | Yes | Implemented |
| P-014 | Unknown/corrupt frame triggers byte-skip + resync and decode continues | `protocol::codec::tests::unknown_frame_resync` | `crates/emwin-protocol/tests/protocol_parity.rs::mixed_corruption_stream` | Yes | Required |
| P-015 | Handler exceptions are isolated | `client::tests::handler_error_isolated` | `N/A (unit coverage)` | No | Implemented |
| P-016 | Reconnect failover rotates endpoints through one pass before delaying | `N/A (runtime behavior)` | `crates/emwin-protocol/tests/qbt_receiver.rs::watchdog_timeout_reconnects_without_termination` | No | Implemented |
| P-017 | Watchdog recovery reconnects without terminating process | `client::watchdog::tests::watchdog_timeout_trigger` | `crates/emwin-protocol/tests/reconnect_failover.rs::watchdog_timeout_reconnects_without_termination` | No | Required |
| P-018 | Decoder remains functional when trailing suffix null bytes are absent | `N/A (unit coverage)` | `crates/emwin-protocol/tests/live_capture_replay.rs::live_capture_replay_manifest_cases` (`mutation-remove-first-suffix-null6`) | No | Required |

## 14. Satellite Draft Alignment Notes

Evidence-backed alignment verdicts from replay and mutation corpus:

1. XOR-FF inbound transform: Confirmed for TCP deployment.
2. Checksum width behavior: Mixed.
   - wire may carry full-sum V1 `CS`
   - receiver validation is low-16 tolerant
3. V2 variable-length `/DL` framing: Confirmed.
4. Compression path behavior: zlib-gated receiver path confirmed.
5. Header/filename strictness beyond draft: inconclusive with current sample set.
6. strict suffix-null requirement: not required for receiver continuity over TCP stream decode.

These are expected transport adaptation differences, not implementation bugs.
