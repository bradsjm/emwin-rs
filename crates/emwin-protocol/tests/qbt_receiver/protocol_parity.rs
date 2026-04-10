//! Protocol regression tests built from representative wire-level scenarios.

use crate::support::{build_frame, build_header, xor_encode};
use emwin_protocol::qbt_receiver::{
    QbtFrameDecoder, QbtFrameEvent, QbtProtocolDecoder, QbtProtocolWarning, calculate_qbt_checksum,
};

#[test]
fn corrupted_stream_recovers_and_keeps_following_valid_frame() {
    let body = [b'B'; 1024];
    let checksum = u32::from(calculate_qbt_checksum(&body));
    let header = build_header("ok.txt", 1, 1, checksum, None);

    let mut decoded = Vec::new();
    decoded.extend_from_slice(b"\0\0\0\0\0\0");
    decoded.extend_from_slice(b"/XXcorrupted");
    decoded.extend_from_slice(b"\0\0\0\0\0\0");
    decoded.extend_from_slice(&header);
    decoded.extend_from_slice(&body);

    let mut decoder = QbtProtocolDecoder::default();
    let events = decoder
        .feed(&xor_encode(&decoded))
        .expect("decoder should resync after unknown frame type");

    assert!(events.iter().any(
        |event| matches!(event, QbtFrameEvent::DataBlock(segment) if segment.filename == "ok.txt")
    ));
}

#[test]
fn full_server_list_frame_ignores_satellite_entries() {
    let payload =
        b"/ServerList/a.example:2211|bad-entry\\ServerList\\/SatServers/sat1:3000+sat2:3001\\SatServers\\\0";
    let mut decoded = Vec::new();
    decoded.extend_from_slice(b"\0\0\0\0\0\0");
    decoded.extend_from_slice(payload);

    let mut decoder = QbtProtocolDecoder::default();
    let events = decoder
        .feed(&xor_encode(&decoded))
        .expect("full server list should parse");

    let server_list = events.iter().find_map(|event| match event {
        QbtFrameEvent::ServerListUpdate(list) => Some(list),
        _ => None,
    });

    let server_list = server_list.expect("expected server list update event");
    assert_eq!(server_list.servers.len(), 1);
}

#[test]
fn checksum_mismatch_drops_invalid_frame_and_preserves_later_valid_frame() {
    let bad_body = [b'X'; 1024];
    let bad_checksum = (u32::from(calculate_qbt_checksum(&bad_body))).wrapping_add(1);
    let bad_header = build_header("bad.bin", 1, 2, bad_checksum, None);
    let bad_frame = build_frame(bad_header, &bad_body);

    let good_body = [b'Y'; 1024];
    let good_checksum = u32::from(calculate_qbt_checksum(&good_body));
    let good_header = build_header("good.bin", 2, 2, good_checksum, None);
    let good_frame = build_frame(good_header, &good_body);

    let mut wire = Vec::new();
    wire.extend_from_slice(&bad_frame);
    wire.extend_from_slice(&good_frame);

    let mut decoder = QbtProtocolDecoder::default();
    let events = decoder
        .feed(&wire)
        .expect("stream should drop checksum mismatch and keep valid frames");

    assert!(events.iter().any(|event| matches!(
        event,
        QbtFrameEvent::Warning(QbtProtocolWarning::ChecksumMismatch { .. })
    )));
    assert!(events.iter().any(|event| {
        matches!(event, QbtFrameEvent::DataBlock(segment) if segment.filename == "good.bin")
    }));
    assert!(!events.iter().any(
        |event| matches!(event, QbtFrameEvent::DataBlock(segment) if segment.filename == "bad.bin")
    ));
}
