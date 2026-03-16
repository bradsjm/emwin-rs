//! NWS text product body parsing module.
//!
//! This module provides parsing for geographic and temporal codes found within
//! NWS text product bodies, including VTEC, UGC, HVTEC, and LAT...LON polygons.

mod cap;
mod enrich;
mod hvtec;
mod latlon;
mod support;
mod time_mot_loc;
mod ugc;
mod vtec;
mod vtec_events;
mod wind_hail;

pub(crate) use enrich::{
    BodyExtractionPlan, BodyInputFormat, body_extraction_plan, enrich_body_from_plan,
    enrich_body_from_plan_with_format,
};
pub use enrich::{BodyExtractorId, GenericBody, ProductBody, enrich_body};
pub use hvtec::{
    HvtecCause, HvtecCode, HvtecRecord, HvtecSeverity, parse_hvtec_codes,
    parse_hvtec_codes_with_issues,
};
pub use latlon::{LatLonPolygon, parse_latlon_polygons, parse_latlon_polygons_with_issues};
pub use time_mot_loc::{
    TimeMotLocEntry, parse_time_mot_loc_entries, parse_time_mot_loc_entries_with_issues,
};
pub use ugc::{
    UgcArea, UgcClass, UgcCode, UgcSection, parse_ugc_sections, parse_ugc_sections_with_issues,
};
pub use vtec::{VtecCode, parse_vtec_codes, parse_vtec_codes_with_issues};
pub use vtec_events::{VtecEventBody, VtecEventSegment};
pub use wind_hail::{
    WindHailEntry, WindHailKind, parse_wind_hail_entries, parse_wind_hail_entries_with_issues,
};
