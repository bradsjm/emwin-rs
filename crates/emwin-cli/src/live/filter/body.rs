use emwin_parser::{HvtecCode, ProductBody, UgcArea, UgcSection, VtecCode, WindHailEntry};
use std::collections::BTreeMap;

pub(crate) fn body_ugc_sections(body: &ProductBody) -> Vec<&UgcSection> {
    match body {
        ProductBody::VtecEvent(body) => body
            .segments
            .iter()
            .flat_map(|segment| segment.ugc_sections.iter())
            .collect(),
        ProductBody::Generic(body) => body
            .ugc
            .iter()
            .flat_map(|sections| sections.iter())
            .collect(),
    }
}

pub(crate) fn body_vtec_codes(body: &ProductBody) -> Vec<&VtecCode> {
    match body {
        ProductBody::VtecEvent(body) => body
            .segments
            .iter()
            .flat_map(|segment| segment.vtec.iter())
            .collect(),
        ProductBody::Generic(_) => Vec::new(),
    }
}

pub(crate) fn body_hvtec_codes(body: &ProductBody) -> Vec<&HvtecCode> {
    match body {
        ProductBody::VtecEvent(body) => body
            .segments
            .iter()
            .flat_map(|segment| segment.hvtec.iter())
            .collect(),
        ProductBody::Generic(_) => Vec::new(),
    }
}

pub(crate) fn body_wind_hail_entries(body: &ProductBody) -> Vec<&WindHailEntry> {
    match body {
        ProductBody::VtecEvent(body) => body
            .segments
            .iter()
            .flat_map(|segment| segment.wind_hail.iter())
            .collect(),
        ProductBody::Generic(body) => body
            .wind_hail
            .iter()
            .flat_map(|entries| entries.iter())
            .collect(),
    }
}

pub(crate) fn body_vtec_codes_len(body: &ProductBody) -> usize {
    match body {
        ProductBody::VtecEvent(body) => {
            body.segments.iter().map(|segment| segment.vtec.len()).sum()
        }
        ProductBody::Generic(_) => 0,
    }
}

pub(crate) fn body_ugc_sections_len(body: &ProductBody) -> usize {
    match body {
        ProductBody::VtecEvent(body) => body
            .segments
            .iter()
            .map(|segment| segment.ugc_sections.len())
            .sum(),
        ProductBody::Generic(body) => body.ugc.as_ref().map_or(0, Vec::len),
    }
}

pub(crate) fn body_hvtec_codes_len(body: &ProductBody) -> usize {
    match body {
        ProductBody::VtecEvent(body) => body
            .segments
            .iter()
            .map(|segment| segment.hvtec.len())
            .sum(),
        ProductBody::Generic(_) => 0,
    }
}

pub(crate) fn body_latlon_len(body: &ProductBody) -> usize {
    match body {
        ProductBody::VtecEvent(body) => body
            .segments
            .iter()
            .map(|segment| segment.polygons.len())
            .sum(),
        ProductBody::Generic(body) => body.latlon.as_ref().map_or(0, Vec::len),
    }
}

pub(crate) fn body_time_mot_loc_len(body: &ProductBody) -> usize {
    match body {
        ProductBody::VtecEvent(body) => body
            .segments
            .iter()
            .map(|segment| segment.time_mot_loc.len())
            .sum(),
        ProductBody::Generic(body) => body.time_mot_loc.as_ref().map_or(0, Vec::len),
    }
}

pub(crate) fn body_wind_hail_len(body: &ProductBody) -> usize {
    match body {
        ProductBody::VtecEvent(body) => body
            .segments
            .iter()
            .map(|segment| segment.wind_hail.len())
            .sum(),
        ProductBody::Generic(body) => body.wind_hail.as_ref().map_or(0, Vec::len),
    }
}

pub(crate) fn matches_geo_states(
    allowed: &Option<std::collections::BTreeSet<String>>,
    sections: &[&UgcSection],
) -> bool {
    match allowed {
        Some(allowed) => sections.iter().any(|section| {
            section.counties.keys().any(|state| allowed.contains(state))
                || section.zones.keys().any(|state| allowed.contains(state))
                || section
                    .fire_zones
                    .keys()
                    .any(|state| allowed.contains(state))
                || section
                    .marine_zones
                    .keys()
                    .any(|state| allowed.contains(state))
        }),
        None => true,
    }
}

pub(crate) fn matches_enriched_ugc_codes(
    allowed: &Option<std::collections::BTreeSet<String>>,
    sections: &[&UgcSection],
    select: fn(&UgcSection) -> &BTreeMap<String, Vec<UgcArea>>,
    class_code: char,
) -> bool {
    match allowed {
        Some(allowed) => sections.iter().any(|section| {
            select(section).iter().any(|(state, areas)| {
                areas
                    .iter()
                    .any(|area| allowed.contains(&format!("{state}{class_code}{:03}", area.id)))
            })
        }),
        None => true,
    }
}
