use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

use super::NwslidEntry;

pub const NWSLID_GENERATED_AT_UTC: &str = "2026-03-06T21:30:23Z";
pub const NWSLID_ENTRY_COUNT: usize = 4158;

#[derive(Debug, Deserialize)]
struct RawNwslidEntry {
    state_code: String,
    stream_name: String,
    proximity: String,
    place_name: String,
    latitude: f64,
    longitude: f64,
}

static NWSLID_CATALOG: OnceLock<Vec<NwslidEntry>> = OnceLock::new();

pub fn catalog() -> &'static [NwslidEntry] {
    NWSLID_CATALOG
        .get_or_init(|| {
            let rows: BTreeMap<String, RawNwslidEntry> =
                serde_json::from_str(include_str!("../../data/nwslid.json"))
                    .expect("NWSLID catalog JSON should be valid");
            rows.into_iter()
                .map(|(nwslid, entry)| NwslidEntry {
                    nwslid: leak(nwslid),
                    state_code: leak(entry.state_code),
                    stream_name: leak(entry.stream_name),
                    proximity: leak(entry.proximity),
                    place_name: leak(entry.place_name),
                    latitude: entry.latitude,
                    longitude: entry.longitude,
                })
                .collect()
        })
        .as_slice()
}

fn leak(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}
