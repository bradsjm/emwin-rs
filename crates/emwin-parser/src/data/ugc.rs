use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

use super::UgcLocationEntry;

pub const UGC_GENERATED_AT_UTC: &str = "2026-03-07T02:10:16Z";
pub const UGC_COUNTY_SOURCE_PATH: &str = "crates/emwin-parser/data/ugc_counties.json";
pub const UGC_ZONE_SOURCE_PATH: &str = "crates/emwin-parser/data/ugc_zones.json";
pub const UGC_COUNTY_ENTRY_COUNT: usize = 3327;
pub const UGC_ZONE_ENTRY_COUNT: usize = 4655;

#[derive(Debug, Deserialize)]
struct RawUgcLocationEntry {
    name: String,
    latitude: f64,
    longitude: f64,
}

static UGC_COUNTY_CATALOG: OnceLock<Vec<UgcLocationEntry>> = OnceLock::new();
static UGC_ZONE_CATALOG: OnceLock<Vec<UgcLocationEntry>> = OnceLock::new();

pub fn county_catalog() -> &'static [UgcLocationEntry] {
    UGC_COUNTY_CATALOG
        .get_or_init(|| parse_catalog(include_str!("../../data/ugc_counties.json")))
        .as_slice()
}

pub fn zone_catalog() -> &'static [UgcLocationEntry] {
    UGC_ZONE_CATALOG
        .get_or_init(|| parse_catalog(include_str!("../../data/ugc_zones.json")))
        .as_slice()
}

fn parse_catalog(raw: &str) -> Vec<UgcLocationEntry> {
    let rows: BTreeMap<String, RawUgcLocationEntry> =
        serde_json::from_str(raw).expect("UGC catalog JSON should be valid");
    rows.into_iter()
        .map(|(code, entry)| UgcLocationEntry {
            code: leak(code),
            name: leak(entry.name),
            latitude: entry.latitude,
            longitude: entry.longitude,
        })
        .collect()
}

fn leak(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}
