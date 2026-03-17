//! Event-oriented VTEC body modeling.
//!
//! VTEC-bearing generic products are modeled as ordered source segments so the
//! recovered VTEC, UGC, geography, and ancillary fields stay correlated.

use crate::body::enrich::ParsedPolygon;
use crate::{
    GeoPoint, HvtecCode, LatLonPolygon, ProductParseIssue, TimeMotLocEntry, UgcSection, VtecCode,
    WindHailEntry, parse_hvtec_codes_with_issues, parse_latlon_polygons_with_issues,
    parse_time_mot_loc_entries_with_issues, parse_ugc_sections_with_issues,
    parse_vtec_codes_with_issues, parse_wind_hail_entries_with_issues, polygon_bounds,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::BTreeSet;

/// Event-oriented body for VTEC-bearing generic products.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VtecEventBody {
    pub segments: Vec<VtecEventSegment>,
}

/// Source segment containing one or more correlated VTEC events.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VtecEventSegment {
    pub segment_index: usize,
    pub vtec: Vec<VtecCode>,
    pub ugc_sections: Vec<UgcSection>,
    pub hvtec: Vec<HvtecCode>,
    pub polygons: Vec<LatLonPolygon>,
    pub time_mot_loc: Vec<TimeMotLocEntry>,
    pub wind_hail: Vec<WindHailEntry>,
}

pub(crate) fn validate_vtec_event_segments(
    segments: &[VtecEventSegment],
) -> Vec<ProductParseIssue> {
    let mut issues = Vec::new();
    for segment in segments {
        issues.extend(validate_vtec_segment(
            segment,
            VtecSegmentQcContext::default(),
        ));
    }
    issues
}

pub(crate) fn parse_vtec_event_body_with_issues(
    text: &str,
    reference_time: Option<DateTime<Utc>>,
) -> (Option<VtecEventBody>, Vec<ProductParseIssue>) {
    let mut segments = Vec::new();
    let mut issues = Vec::new();

    for (segment_index, raw_segment) in split_vtec_segments(text).into_iter().enumerate() {
        let (vtec, mut segment_issues) = parse_vtec_codes_with_issues(&raw_segment);
        if vtec.is_empty() {
            issues.append(&mut segment_issues);
            continue;
        }

        let mut qc_context = VtecSegmentQcContext::default();
        let ugc_sections = match reference_time {
            Some(reference_time) => {
                let (parsed, mut parse_issues) =
                    parse_ugc_sections_with_issues(&raw_segment, reference_time);
                segment_issues.append(&mut parse_issues);
                parsed
            }
            None => {
                if raw_segment.lines().any(is_ugc_start) {
                    qc_context.ugc_blocked_by_missing_reference_time = true;
                    segment_issues.push(ProductParseIssue::new(
                        "ugc_parse",
                        "missing_reference_time",
                        format!(
                            "segment {segment_index} could not parse UGC sections because the header timestamp could not be resolved"
                        ),
                        None,
                    ));
                }
                Vec::new()
            }
        };

        let (hvtec, mut hvtec_issues) = parse_hvtec_codes_with_issues(&raw_segment);
        segment_issues.append(&mut hvtec_issues);

        let (polygons, mut polygon_issues) = parse_latlon_polygons_with_issues(&raw_segment);
        segment_issues.append(&mut polygon_issues);

        let time_mot_loc = match reference_time {
            Some(reference_time) => {
                let (parsed, mut parse_issues) =
                    parse_time_mot_loc_entries_with_issues(&raw_segment, reference_time);
                segment_issues.append(&mut parse_issues);
                parsed
            }
            None => {
                if raw_segment.lines().any(line_has_time_mot_loc) {
                    qc_context.time_mot_loc_blocked_by_missing_reference_time = true;
                    segment_issues.push(ProductParseIssue::new(
                        "time_mot_loc_parse",
                        "missing_reference_time",
                        format!(
                            "segment {segment_index} could not parse TIME...MOT...LOC entries because the header timestamp could not be resolved"
                        ),
                        None,
                    ));
                }
                Vec::new()
            }
        };

        let (wind_hail, mut wind_hail_issues) = parse_wind_hail_entries_with_issues(&raw_segment);
        segment_issues.append(&mut wind_hail_issues);

        let segment = VtecEventSegment {
            segment_index,
            vtec,
            ugc_sections,
            hvtec,
            polygons,
            time_mot_loc,
            wind_hail,
        };

        segment_issues.extend(validate_vtec_segment(&segment, qc_context));
        issues.append(&mut segment_issues);
        segments.push(segment);
    }

    (
        (!segments.is_empty()).then_some(VtecEventBody { segments }),
        issues,
    )
}

fn split_vtec_segments(text: &str) -> Vec<String> {
    let normalized = text.replace('\r', "");
    let mut segments = Vec::new();
    let mut current = Vec::new();
    let mut current_has_vtec = false;
    let mut current_has_geography = false;

    for line in normalized.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim() == "$$" {
            push_vtec_segment(&mut segments, &mut current);
            current_has_vtec = false;
            current_has_geography = false;
            continue;
        }

        let boundary = !current.is_empty()
            && current_has_vtec
            && (is_ugc_start(trimmed) || (line_has_vtec(trimmed) && current_has_geography));
        if boundary {
            push_vtec_segment(&mut segments, &mut current);
            current_has_vtec = false;
            current_has_geography = false;
        }

        if current.is_empty() && !looks_like_event_content(trimmed) {
            continue;
        }

        if trimmed.trim().is_empty() && current.is_empty() {
            continue;
        }

        current.push(trimmed.to_string());
        current_has_vtec |= line_has_vtec(trimmed);
        current_has_geography |= line_has_geography(trimmed);
    }

    push_vtec_segment(&mut segments, &mut current);
    segments
}

fn push_vtec_segment(segments: &mut Vec<String>, current: &mut Vec<String>) {
    if current.is_empty() {
        return;
    }
    let joined = current.join("\n");
    if !parse_vtec_codes_with_issues(&joined).0.is_empty() {
        segments.push(joined);
    }
    current.clear();
}

fn looks_like_event_content(line: &str) -> bool {
    is_ugc_start(line)
        || line_has_vtec(line)
        || line_has_geography(line)
        || line_has_wind_hail(line)
        || !parse_hvtec_codes_with_issues(line).0.is_empty()
}

fn line_has_vtec(line: &str) -> bool {
    !parse_vtec_codes_with_issues(line).0.is_empty()
}

fn line_has_geography(line: &str) -> bool {
    let upper = line.trim_start().to_ascii_uppercase();
    upper.starts_with("LAT...LON") || upper.starts_with("TIME...MOT...LOC")
}

fn line_has_time_mot_loc(line: &str) -> bool {
    line.trim_start()
        .to_ascii_uppercase()
        .starts_with("TIME...MOT...LOC")
}

fn line_has_wind_hail(line: &str) -> bool {
    let upper = line.trim_start().to_ascii_uppercase();
    upper.starts_with("HAILTHREAT")
        || upper.starts_with("MAXHAILSIZE")
        || upper.starts_with("WINDTHREAT")
        || upper.starts_with("MAXWINDGUST")
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct VtecSegmentQcContext {
    ugc_blocked_by_missing_reference_time: bool,
    time_mot_loc_blocked_by_missing_reference_time: bool,
}

fn validate_vtec_segment(
    segment: &VtecEventSegment,
    context: VtecSegmentQcContext,
) -> Vec<ProductParseIssue> {
    let mut issues = Vec::new();

    if segment.ugc_sections.is_empty() && !context.ugc_blocked_by_missing_reference_time {
        issues.push(ProductParseIssue::new(
            "body_qc",
            "vtec_segment_missing_ugc",
            format!(
                "segment {} parsed VTEC content but did not recover UGC sections from the source text",
                segment.segment_index
            ),
            None,
        ));
    }

    if segment.polygons.is_empty()
        && !segment_has_marine_only_ugc(segment)
        && !segment_is_watch_only(segment)
    {
        issues.push(ProductParseIssue::new(
            "body_qc",
            "vtec_segment_missing_required_polygon",
            format!(
                "segment {} parsed VTEC content but did not recover a LAT...LON polygon from the source text",
                segment.segment_index
            ),
            None,
        ));
    }

    let duplicates = find_segment_duplicate_ugc_codes(segment);
    if !duplicates.is_empty() {
        issues.push(ProductParseIssue::new(
            "body_qc",
            "vtec_segment_duplicate_ugc_code",
            format!(
                "segment {} encountered duplicated UGC codes: {}",
                segment.segment_index,
                duplicates.join(", ")
            ),
            None,
        ));
    }

    issues
}

fn find_segment_duplicate_ugc_codes(segment: &VtecEventSegment) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut duplicates = BTreeSet::new();
    for section in &segment.ugc_sections {
        collect_duplicate_ugc_codes(&section.counties, 'C', &mut seen, &mut duplicates);
        collect_duplicate_ugc_codes(&section.zones, 'Z', &mut seen, &mut duplicates);
        collect_duplicate_ugc_codes(&section.fire_zones, 'F', &mut seen, &mut duplicates);
        collect_duplicate_ugc_codes(&section.marine_zones, 'M', &mut seen, &mut duplicates);
    }
    duplicates.into_iter().collect()
}

fn collect_duplicate_ugc_codes(
    groups: &std::collections::BTreeMap<String, Vec<crate::UgcArea>>,
    class_code: char,
    seen: &mut BTreeSet<String>,
    duplicates: &mut BTreeSet<String>,
) {
    for (state, areas) in groups {
        for area in areas {
            let canonical = format!("{state}{class_code}{:03}", area.id);
            if !seen.insert(canonical.clone()) {
                duplicates.insert(canonical);
            }
        }
    }
}

fn is_ugc_start(line: &str) -> bool {
    let trimmed = line.trim_start();
    let mut chars = trimmed.chars();
    let is_prefix_match = matches!(
        (
            chars.next(),
            chars.next(),
            chars.next(),
            chars.next(),
            chars.next(),
            chars.next()
        ),
        (Some(a), Some(b), Some(class), Some(d1), Some(d2), Some(d3))
            if a.is_ascii_uppercase()
                && b.is_ascii_uppercase()
                && matches!(class, 'C' | 'Z' | 'F' | 'M')
                && d1.is_ascii_digit()
                && d2.is_ascii_digit()
                && d3.is_ascii_digit()
    );
    is_prefix_match
        && trimmed
            .as_bytes()
            .get(6)
            .is_none_or(|character| matches!(character, b'-' | b'>' | b',' | b' ' | b'\t'))
}

fn segment_has_marine_only_ugc(segment: &VtecEventSegment) -> bool {
    !segment.ugc_sections.is_empty()
        && segment.ugc_sections.iter().all(|section| {
            section.counties.is_empty()
                && section.zones.is_empty()
                && section.fire_zones.is_empty()
                && !section.marine_zones.is_empty()
        })
}

fn segment_is_watch_only(segment: &VtecEventSegment) -> bool {
    !segment.vtec.is_empty() && segment.vtec.iter().all(|code| code.significance == 'A')
}

pub(crate) fn vtec_event_body_has_marine_only_ugc(body: &VtecEventBody) -> bool {
    !body.segments.is_empty() && body.segments.iter().all(segment_has_marine_only_ugc)
}

pub(crate) fn vtec_event_body_iter_location_points(
    body: &VtecEventBody,
) -> impl Iterator<Item = GeoPoint> + '_ {
    body.segments.iter().flat_map(|segment| {
        let time_mot_loc = segment
            .time_mot_loc
            .iter()
            .flat_map(|entry| entry.points.iter().map(|&(lat, lon)| GeoPoint { lat, lon }));
        let ugc = segment.ugc_sections.iter().flat_map(|section| {
            section
                .counties
                .values()
                .chain(section.zones.values())
                .chain(section.fire_zones.values())
                .chain(section.marine_zones.values())
                .flat_map(|areas| areas.iter())
                .filter_map(|area| {
                    area.lat
                        .zip(area.lon)
                        .map(|(lat, lon)| GeoPoint { lat, lon })
                })
        });
        let hvtec = segment.hvtec.iter().filter_map(|code| {
            code.location.map(|location| GeoPoint {
                lat: location.latitude,
                lon: location.longitude,
            })
        });

        time_mot_loc.chain(ugc).chain(hvtec)
    })
}

pub(crate) fn vtec_event_body_iter_polygons(
    body: &VtecEventBody,
) -> impl Iterator<Item = ParsedPolygon<'_>> + '_ {
    body.segments.iter().flat_map(|segment| {
        segment.polygons.iter().map(|polygon| ParsedPolygon {
            points: &polygon.points,
            bounds: polygon_bounds(&polygon.points),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::parse_vtec_event_body_with_issues;
    use chrono::Utc;

    #[test]
    fn single_segment_warning_yields_one_segment() {
        let text = "NEC001-051300-\n/O.NEW.KOAX.SV.W.0001.250305T1200Z-250305T1800Z/\nLAT...LON 4143 9613 4145 9610 4140 9608 4138 9612\n";
        let (body, issues) = parse_vtec_event_body_with_issues(text, Some(Utc::now()));
        assert!(issues.is_empty());
        assert_eq!(body.expect("body").segments.len(), 1);
    }

    #[test]
    fn multi_segment_flood_product_yields_multiple_segments() {
        let text = "NCC101-051300-\n/O.NEW.KRAH.FL.W.0001.250305T1200Z-250305T1800Z/\nLAT...LON 3554 7829 3548 7834 3544 7829 3541 7833\n$$\nNCC101-051500-\n/O.CON.KRAH.FL.W.0001.250305T1800Z-250306T0200Z/\nLAT...LON 3554 7829 3548 7834 3544 7829 3541 7833\n";
        let (body, _issues) = parse_vtec_event_body_with_issues(text, Some(Utc::now()));
        assert_eq!(body.expect("body").segments.len(), 2);
    }

    #[test]
    fn segment_with_multiple_vtec_codes_keeps_both_vtec_entries() {
        let text = "/O.NEW.KOAX.SV.W.0001.250305T1200Z-250305T1800Z//O.NEW.KOAX.TO.W.0002.250305T1200Z-250305T1800Z/\nLAT...LON 4143 9613 4145 9610 4140 9608 4138 9612\n";
        let (body, _issues) = parse_vtec_event_body_with_issues(text, Some(Utc::now()));
        assert_eq!(body.expect("body").segments[0].vtec.len(), 2);
    }

    #[test]
    fn marine_only_ugc_skips_missing_polygon_issue() {
        let text = "AMZ250-051300-\n/O.NEW.KKEY.MA.W.0001.250305T1200Z-250305T1800Z/\n";
        let (_body, issues) = parse_vtec_event_body_with_issues(text, Some(Utc::now()));
        assert!(
            !issues
                .iter()
                .any(|issue| issue.code == "vtec_segment_missing_required_polygon")
        );
    }

    #[test]
    fn watch_only_segments_skip_missing_polygon_issue() {
        let text = "ALC023-025-047-120700-\n/O.CON.KWNS.TO.A.0048.000000T0000Z-260312T0700Z/\nAL\n.    ALABAMA COUNTIES INCLUDED ARE\n\nCHOCTAW CLARKE DALLAS\n";
        let (_body, issues) = parse_vtec_event_body_with_issues(text, Some(Utc::now()));
        assert!(
            !issues
                .iter()
                .any(|issue| issue.code == "vtec_segment_missing_required_polygon")
        );
    }

    #[test]
    fn missing_ugc_issue_is_emitted() {
        let text = "/O.NEW.KOAX.SV.W.0001.250305T1200Z-250305T1800Z/\nLAT...LON 4143 9613 4145 9610 4140 9608 4138 9612\n";
        let (_body, issues) = parse_vtec_event_body_with_issues(text, Some(Utc::now()));
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "vtec_segment_missing_ugc")
        );
    }

    #[test]
    fn missing_reference_time_still_keeps_vtec_segment() {
        let text = "NEC001-051300-\n/O.NEW.KOAX.SV.W.0001.250305T1200Z-250305T1800Z/\nTIME...MOT...LOC 1200Z 300DEG 25KT 4143 9613\n";
        let (body, issues) = parse_vtec_event_body_with_issues(text, None);
        assert_eq!(body.expect("body").segments.len(), 1);
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "missing_reference_time")
        );
    }

    #[test]
    fn missing_reference_time_with_ugc_does_not_emit_missing_ugc_qc() {
        let text = "NEC001-051300-\n/O.NEW.KOAX.SV.W.0001.250305T1200Z-250305T1800Z/\nLAT...LON 4143 9613 4145 9610 4140 9608 4138 9612\n";
        let (body, issues) = parse_vtec_event_body_with_issues(text, None);
        assert_eq!(body.expect("body").segments.len(), 1);
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "missing_reference_time")
        );
        assert!(
            !issues
                .iter()
                .any(|issue| issue.code == "vtec_segment_missing_ugc")
        );
    }

    #[test]
    fn missing_reference_time_without_ugc_candidate_emits_missing_ugc_qc() {
        let text = "/O.NEW.KOAX.SV.W.0001.250305T1200Z-250305T1800Z/\nLAT...LON 4143 9613 4145 9610 4140 9608 4138 9612\n";
        let (body, issues) = parse_vtec_event_body_with_issues(text, None);
        assert_eq!(body.expect("body").segments.len(), 1);
        assert!(
            !issues
                .iter()
                .any(|issue| issue.code == "missing_reference_time")
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "vtec_segment_missing_ugc")
        );
    }

    #[test]
    fn duplicate_ugc_across_two_sections_in_same_segment_emits_duplicate_issue() {
        let text = "\
NCC101-051300-
NCC101-051300-
/O.NEW.KRAH.FL.W.0001.250305T1200Z-250305T1800Z/
LAT...LON 3554 7829 3548 7834 3544 7829 3541 7833
";
        let (_body, issues) = parse_vtec_event_body_with_issues(text, Some(Utc::now()));
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "vtec_segment_duplicate_ugc_code")
        );
    }

    #[test]
    fn contiguous_vtec_lines_after_single_ugc_stay_in_same_segment() {
        let text = "LEZ040-041-120800-\n/O.CON.KBUF.SC.Y.0019.000000T0000Z-260312T2100Z/\n/O.NEW.KBUF.GL.A.0006.260313T1600Z-260314T0800Z/\n";
        let (body, _issues) = parse_vtec_event_body_with_issues(text, Some(Utc::now()));
        assert_eq!(body.expect("body").segments.len(), 1);
    }

    #[test]
    fn new_vtec_after_geography_starts_new_segment() {
        let text = "IAC001-051300-\n/O.NEW.KDMX.TO.W.0001.250305T1200Z-250305T1800Z/\nLAT...LON 4143 9613 4145 9610 4140 9608 4138 9612\n/O.NEW.KDMX.SV.W.0002.250305T1205Z-250305T1815Z/\nLAT...LON 4142 9612 4144 9609 4139 9607 4137 9611\n";
        let (body, _issues) = parse_vtec_event_body_with_issues(text, Some(Utc::now()));
        assert_eq!(body.expect("body").segments.len(), 2);
    }
}
