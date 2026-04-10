//! Property tests for decoder robustness and chunking behavior.

use crate::support::build_single_block_frame;
use emwin_protocol::qbt_receiver::{QbtFrameDecoder, QbtFrameEvent, QbtProtocolDecoder};
use proptest::prelude::*;

fn summary(events: &[QbtFrameEvent]) -> Vec<(String, usize)> {
    events
        .iter()
        .map(|evt| match evt {
            QbtFrameEvent::DataBlock(seg) => ("data".to_string(), seg.content.len()),
            QbtFrameEvent::ServerListUpdate(list) => ("servers".to_string(), list.servers.len()),
            QbtFrameEvent::Warning(_) => ("warning".to_string(), 0),
            _ => ("unknown".to_string(), 0),
        })
        .collect()
}

proptest! {
    #[test]
    fn random_wire_input_never_panics(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let mut decoder = QbtProtocolDecoder::default();
        let _ = decoder.feed(&data);
    }

    #[test]
    fn chunked_vs_single_feed_equivalence(split in 0usize..1100usize) {
        let body = vec![b'X'; 1024];
        let wire = build_single_block_frame("prop.txt", &body);
        let split_idx = split.min(wire.len());

        let mut single = QbtProtocolDecoder::default();
        let single_events = single.feed(&wire).expect("single decode must succeed");

        let mut chunked = QbtProtocolDecoder::default();
        let mut events = Vec::new();
        events.extend(chunked.feed(&wire[..split_idx]).expect("first chunk decode must succeed"));
        events.extend(chunked.feed(&wire[split_idx..]).expect("second chunk decode must succeed"));

        prop_assert_eq!(summary(&single_events), summary(&events));
    }
}
