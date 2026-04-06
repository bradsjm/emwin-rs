use emwin_parser::{
    BbbKind, HvtecCause, HvtecRecord, HvtecSeverity, ProductEnrichmentSource, WindHailKind,
};
use std::collections::BTreeSet;

/// Raw filter parameters collected from CLI flags or HTTP query strings.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct FileFilterInput {
    pub(crate) filename: Option<String>,
    pub(crate) source: Option<String>,
    pub(crate) pil: Option<String>,
    pub(crate) family: Option<String>,
    pub(crate) container: Option<String>,
    pub(crate) wmo_prefix: Option<String>,
    pub(crate) office: Option<String>,
    pub(crate) office_city: Option<String>,
    pub(crate) office_state: Option<String>,
    pub(crate) bbb_kind: Option<String>,
    pub(crate) cccc: Option<String>,
    pub(crate) ttaaii: Option<String>,
    pub(crate) afos: Option<String>,
    pub(crate) bbb: Option<String>,
    pub(crate) has_issues: Option<String>,
    pub(crate) issue_kind: Option<String>,
    pub(crate) issue_code: Option<String>,
    pub(crate) has_vtec: Option<String>,
    pub(crate) has_ugc: Option<String>,
    pub(crate) has_hvtec: Option<String>,
    pub(crate) has_latlon: Option<String>,
    pub(crate) has_time_mot_loc: Option<String>,
    pub(crate) has_wind_hail: Option<String>,
    pub(crate) state: Option<String>,
    pub(crate) county: Option<String>,
    pub(crate) zone: Option<String>,
    pub(crate) fire_zone: Option<String>,
    pub(crate) marine_zone: Option<String>,
    pub(crate) vtec_phenomena: Option<String>,
    pub(crate) vtec_significance: Option<String>,
    pub(crate) vtec_action: Option<String>,
    pub(crate) vtec_office: Option<String>,
    pub(crate) etn: Option<String>,
    pub(crate) hvtec_nwslid: Option<String>,
    pub(crate) hvtec_severity: Option<String>,
    pub(crate) hvtec_cause: Option<String>,
    pub(crate) hvtec_record: Option<String>,
    pub(crate) wind_hail_kind: Option<String>,
    pub(crate) lat: Option<f64>,
    pub(crate) lon: Option<f64>,
    pub(crate) distance_miles: Option<f64>,
    pub(crate) min_lat: Option<f64>,
    pub(crate) max_lat: Option<f64>,
    pub(crate) min_lon: Option<f64>,
    pub(crate) max_lon: Option<f64>,
    pub(crate) min_wind_mph: Option<f64>,
    pub(crate) min_hail_inches: Option<f64>,
    pub(crate) min_size: Option<usize>,
    pub(crate) max_size: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileFilterInputError {
    pub(crate) message: String,
}

impl FileFilterInputError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

pub(crate) fn matches_option_set(
    allowed: &Option<BTreeSet<String>>,
    value: Option<&str>,
    normalize: fn(&str) -> String,
) -> bool {
    match allowed {
        Some(allowed) => value
            .map(normalize)
            .map(|normalized| allowed.contains(&normalized))
            .unwrap_or(false),
        None => true,
    }
}

pub(crate) fn matches_number_set(allowed: &Option<BTreeSet<u32>>, value: u32) -> bool {
    match allowed {
        Some(allowed) => allowed.contains(&value),
        None => true,
    }
}

pub(crate) fn matches_serialized_option<T: Copy>(
    allowed: &Option<BTreeSet<String>>,
    value: Option<T>,
    serialize: fn(T) -> &'static str,
) -> bool {
    match allowed {
        Some(allowed) => value
            .map(serialize)
            .map(|serialized| allowed.contains(serialized))
            .unwrap_or(false),
        None => true,
    }
}

pub(crate) fn matches_optional_presence(expected: Option<bool>, value_count: usize) -> bool {
    match expected {
        Some(expected) => expected == (value_count > 0),
        None => true,
    }
}

pub(crate) fn csv_values(
    raw: Option<&str>,
    normalize: fn(&str) -> String,
) -> Option<BTreeSet<String>> {
    let values = raw
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(normalize)
        .collect::<BTreeSet<_>>();

    (!values.is_empty()).then_some(values)
}

pub(crate) fn csv_numbers(raw: Option<&str>) -> Option<BTreeSet<u32>> {
    let values = raw
        .into_iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter_map(|value| value.parse::<u32>().ok())
        .collect::<BTreeSet<_>>();

    (!values.is_empty()).then_some(values)
}

pub(crate) fn parse_optional_bool(raw: Option<&str>) -> Option<bool> {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.eq_ignore_ascii_case("true") || value == "1" => Some(true),
        Some(value) if value.eq_ignore_ascii_case("false") || value == "0" => Some(false),
        _ => None,
    }
}

pub(crate) fn product_source_name(value: ProductEnrichmentSource) -> &'static str {
    match value {
        ProductEnrichmentSource::TextHeader => "text_header",
        ProductEnrichmentSource::WmoBulletin => "wmo_bulletin",
        ProductEnrichmentSource::FilenameNonText => "filename_non_text",
        ProductEnrichmentSource::Unknown => "unknown",
    }
}

pub(crate) fn bbb_kind_name(value: BbbKind) -> &'static str {
    match value {
        BbbKind::Amendment => "amendment",
        BbbKind::Correction => "correction",
        BbbKind::DelayedRepeat => "delayed_repeat",
        BbbKind::Other => "other",
    }
}

pub(crate) fn hvtec_severity_name(value: HvtecSeverity) -> &'static str {
    match value {
        HvtecSeverity::None => "none",
        HvtecSeverity::Minor => "minor",
        HvtecSeverity::Moderate => "moderate",
        HvtecSeverity::Major => "major",
        HvtecSeverity::Unknown => "unknown",
    }
}

pub(crate) fn hvtec_cause_name(value: HvtecCause) -> &'static str {
    match value {
        HvtecCause::ExcessiveRainfall => "excessive_rainfall",
        HvtecCause::Snowmelt => "snowmelt",
        HvtecCause::RainAndSnowmelt => "rain_and_snowmelt",
        HvtecCause::DamFailure => "dam_failure",
        HvtecCause::GlacierOutburst => "glacier_outburst",
        HvtecCause::IceJam => "ice_jam",
        HvtecCause::RainSnowmeltIceJam => "rain_snowmelt_ice_jam",
        HvtecCause::UpstreamFloodingStormSurge => "upstream_flooding_storm_surge",
        HvtecCause::UpstreamFloodingTidalEffects => "upstream_flooding_tidal_effects",
        HvtecCause::ElevatedUpstreamFlowTidalEffects => "elevated_upstream_flow_tidal_effects",
        HvtecCause::WindTidalEffects => "wind_tidal_effects",
        HvtecCause::UpstreamDamRelease => "upstream_dam_release",
        HvtecCause::MultipleCauses => "multiple_causes",
        HvtecCause::OtherEffects => "other_effects",
        HvtecCause::Unknown => "unknown",
        HvtecCause::Other => "other",
    }
}

pub(crate) fn hvtec_record_name(value: HvtecRecord) -> &'static str {
    match value {
        HvtecRecord::NoRecord => "no_record",
        HvtecRecord::NearRecord => "near_record",
        HvtecRecord::NotApplicable => "not_applicable",
        HvtecRecord::Unavailable => "unavailable",
        HvtecRecord::Unknown => "unknown",
    }
}

pub(crate) fn wind_hail_kind_name(value: WindHailKind) -> &'static str {
    match value {
        WindHailKind::LegacyWind => "legacy_wind",
        WindHailKind::LegacyHail => "legacy_hail",
        WindHailKind::WindThreat => "wind_threat",
        WindHailKind::MaxWindGust => "max_wind_gust",
        WindHailKind::HailThreat => "hail_threat",
        WindHailKind::MaxHailSize => "max_hail_size",
    }
}

pub(crate) fn is_wind_entry(entry: &emwin_parser::WindHailEntry) -> bool {
    matches!(
        entry.kind,
        WindHailKind::LegacyWind | WindHailKind::MaxWindGust
    )
}

pub(crate) fn is_hail_entry(entry: &emwin_parser::WindHailEntry) -> bool {
    matches!(
        entry.kind,
        WindHailKind::LegacyHail | WindHailKind::MaxHailSize
    )
}

pub(crate) fn wind_speed_mph(value: f64, units: &str) -> f64 {
    match normalize_upper(units).as_str() {
        "KTS" | "KT" => value * 1.150_78,
        _ => value,
    }
}

pub(crate) fn normalize_upper(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

pub(crate) fn normalize_lower(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::FileFilterInput;
    use crate::live::filter::FileEventFilter;

    #[test]
    fn input_validation_rejects_distance_without_coords() {
        let err = FileEventFilter::try_from_input(&FileFilterInput {
            distance_miles: Some(5.0),
            ..FileFilterInput::default()
        })
        .expect_err("distance without coords should fail");

        assert_eq!(err.message, "distance_miles requires both lat and lon");
    }

    #[test]
    fn input_validation_rejects_partial_bbox() {
        let err = FileEventFilter::try_from_input(&FileFilterInput {
            min_lat: Some(40.0),
            max_lat: Some(42.0),
            min_lon: Some(-97.0),
            ..FileFilterInput::default()
        })
        .expect_err("partial bbox should fail");

        assert_eq!(
            err.message,
            "min_lat, max_lat, min_lon, and max_lon must be provided together"
        );
    }
}
