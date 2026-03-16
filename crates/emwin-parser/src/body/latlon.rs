//! NWS LAT...LON polygon parsing module.
//!
//! LAT...LON blocks define geographic warning areas as polygons using
//! coordinate pairs. Coordinates use several packed formats that appear in
//! wrapped and occasionally damaged source text, so the parser preserves the
//! existing narrow repair heuristics for split and fused tokens.

use serde::ser::{SerializeSeq, Serializer};

use crate::ProductParseIssue;
use crate::body::support::{ascii_find_case_insensitive, format_polygon_wkt};

const MARKER: &str = "LAT...LON";

/// A parsed LAT...LON polygon.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct LatLonPolygon {
    /// Coordinate pairs as (latitude, longitude) in decimal degrees
    #[serde(serialize_with = "serialize_points")]
    pub points: Vec<(f64, f64)>,
    /// PostGIS WKT (Well-Known Text) representation
    pub wkt: String,
}

impl LatLonPolygon {
    pub(crate) fn from_points(points: Vec<(f64, f64)>) -> Result<Self, ProductParseIssue> {
        let points = validate_polygon(points, "CAP polygon", &mut Vec::new())?;
        Ok(Self {
            wkt: format_polygon_wkt(&points),
            points,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LatLonCandidate<'a> {
    raw: String,
    source_line: &'a str,
}

/// Parses all LAT...LON polygons found in the given text.
pub fn parse_latlon_polygons(text: &str) -> Vec<LatLonPolygon> {
    parse_latlon_polygons_with_issues(text).0
}

pub fn parse_latlon_polygons_with_issues(
    text: &str,
) -> (Vec<LatLonPolygon>, Vec<ProductParseIssue>) {
    let mut polygons = Vec::new();
    let mut issues = Vec::new();

    for candidate in find_latlon_candidates(text) {
        let (polygon, parse_issues) = parse_latlon_candidate(&candidate);
        if let Some(polygon) = polygon {
            polygons.push(polygon);
        }
        issues.extend(parse_issues);
    }

    (polygons, issues)
}

fn find_latlon_candidates(text: &str) -> Vec<LatLonCandidate<'_>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut candidates = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        let Some(marker_position) = ascii_find_case_insensitive(line, MARKER) else {
            index += 1;
            continue;
        };

        let (raw, consumed) = collect_latlon_candidate(&lines, index, marker_position);
        index += consumed.max(1);
        if let Some(raw) = raw {
            candidates.push(LatLonCandidate {
                raw,
                source_line: line,
            });
        }
    }

    candidates
}

fn collect_latlon_candidate(
    lines: &[&str],
    start_index: usize,
    marker_position: usize,
) -> (Option<String>, usize) {
    let mut raw = String::new();
    raw.push_str(lines[start_index][marker_position..].trim());
    let mut consumed = 1;

    while start_index + consumed < lines.len() {
        let next_line = lines[start_index + consumed].trim();
        if next_line.is_empty() || ascii_find_case_insensitive(next_line, MARKER).is_some() {
            break;
        }
        if starts_new_section_header(next_line) && !looks_like_coordinate_line(next_line) {
            break;
        }

        if !raw.is_empty() {
            raw.push(' ');
        }
        raw.push_str(next_line);
        consumed += 1;

        if marker_has_coordinates(&raw) {
            if start_index + consumed >= lines.len() {
                break;
            }
            let peek = lines[start_index + consumed].trim();
            if peek.is_empty()
                || ascii_find_case_insensitive(peek, MARKER).is_some()
                || (starts_new_section_header(peek) && !looks_like_coordinate_line(peek))
            {
                break;
            }
        }
    }

    (Some(raw), consumed)
}

fn marker_has_coordinates(raw: &str) -> bool {
    let mut tokens = raw.split_whitespace();
    matches!(
        (tokens.next(), tokens.next()),
        (Some(marker), Some(token))
            if marker.eq_ignore_ascii_case(MARKER)
                && token
                    .trim_start_matches('-')
                    .chars()
                    .all(|ch| ch.is_ascii_digit())
    )
}

fn parse_latlon_candidate(
    candidate: &LatLonCandidate<'_>,
) -> (Option<LatLonPolygon>, Vec<ProductParseIssue>) {
    let raw = candidate.raw.trim();
    let mut tokens = raw.split_whitespace();
    let Some(marker) = tokens.next() else {
        return (None, vec![invalid_format_issue(raw)]);
    };
    if !marker.eq_ignore_ascii_case(MARKER) {
        return (None, vec![invalid_format_issue(raw)]);
    }

    let raw_coords: Vec<&str> = tokens.collect();
    let Some((coords, mut issues)) = normalize_coordinate_tokens(&raw_coords) else {
        return (
            None,
            vec![ProductParseIssue::new(
                "latlon_parse",
                "invalid_latlon_coordinate_format",
                format!("could not normalize LAT...LON coordinates from block: `{raw}`"),
                Some(raw.to_string()),
            )],
        );
    };

    let points = match parse_points(&coords, raw) {
        Ok(points) => points,
        Err(issue) => {
            issues.push(issue);
            return (None, issues);
        }
    };
    let points = match validate_polygon(points, raw, &mut issues) {
        Ok(points) => points,
        Err(issue) => {
            issues.push(issue);
            return (None, issues);
        }
    };

    (
        Some(LatLonPolygon {
            wkt: format_polygon_wkt(&points),
            points,
        }),
        issues,
    )
}

fn invalid_format_issue(raw: &str) -> ProductParseIssue {
    ProductParseIssue::new(
        "latlon_parse",
        "invalid_latlon_format",
        format!("could not parse LAT...LON block: `{raw}`"),
        Some(raw.to_string()),
    )
}

fn parse_points(coords: &[String], raw: &str) -> Result<Vec<(f64, f64)>, ProductParseIssue> {
    let mut points = Vec::with_capacity(coords.len() / 2);
    let mut pending_latitude: Option<f64> = None;

    for coord in coords {
        if coord.len() == 8 && !coord.starts_with('-') {
            if pending_latitude.is_some() {
                return Err(ProductParseIssue::new(
                    "latlon_parse",
                    "invalid_latlon_coordinate_count",
                    format!("LAT...LON block has an invalid coordinate count: `{raw}`"),
                    Some(raw.to_string()),
                ));
            }

            let lat = parse_coordinate(&coord[..4], true).ok_or_else(|| {
                ProductParseIssue::new(
                    "latlon_parse",
                    "invalid_latlon_latitude",
                    format!("could not parse LAT...LON latitude from block: `{raw}`"),
                    Some(raw.to_string()),
                )
            })?;
            let lon = parse_coordinate(&coord[4..], false).ok_or_else(|| {
                ProductParseIssue::new(
                    "latlon_parse",
                    "invalid_latlon_longitude",
                    format!("could not parse LAT...LON longitude from block: `{raw}`"),
                    Some(raw.to_string()),
                )
            })?;
            points.push((lat, lon));
            continue;
        }

        let value = parse_coordinate(coord, pending_latitude.is_none()).ok_or_else(|| {
            ProductParseIssue::new(
                "latlon_parse",
                if pending_latitude.is_none() {
                    "invalid_latlon_latitude"
                } else {
                    "invalid_latlon_longitude"
                },
                format!(
                    "could not parse LAT...LON {} from block: `{raw}`",
                    if pending_latitude.is_none() {
                        "latitude"
                    } else {
                        "longitude"
                    }
                ),
                Some(raw.to_string()),
            )
        })?;

        if let Some(lat) = pending_latitude.take() {
            points.push((lat, value));
        } else {
            pending_latitude = Some(value);
        }
    }

    if pending_latitude.is_some() || points.len() < 3 {
        return Err(ProductParseIssue::new(
            "latlon_parse",
            "invalid_latlon_coordinate_count",
            format!("LAT...LON block has an invalid coordinate count: `{raw}`"),
            Some(raw.to_string()),
        ));
    }

    Ok(points)
}

fn normalize_coordinate_tokens(tokens: &[&str]) -> Option<(Vec<String>, Vec<ProductParseIssue>)> {
    let mut normalized = Vec::new();
    let mut issues = Vec::new();
    let mut index = 0;

    while index < tokens.len() {
        if is_latlon_terminator(tokens[index]) {
            break;
        }
        let token = consume_coordinate_token(tokens, &mut index)?;
        if let Some((lat, lon)) = split_packed_coordinate_pair(&normalized, &token) {
            normalized.push(lat);
            normalized.push(lon);
            continue;
        }
        // Some warning products lose an internal separator and fuse a longitude
        // token with the following latitude token, e.g. `98112979` instead of
        // `9811 2979`. Only repair the narrow 4+4 case when the stream is
        // already in pairwise 4/5-digit mode and a longitude is definitively
        // expected next.
        if let Some((lon, lat)) = split_fused_coordinate_token(&normalized, &token) {
            issues.push(ProductParseIssue::new(
                "latlon_parse",
                "latlon_fused_token_repaired",
                format!("repaired fused LAT...LON coordinate token `{token}` into `{lon}` `{lat}`"),
                Some(token),
            ));
            normalized.push(lon);
            normalized.push(lat);
            continue;
        }
        normalized.push(token);
    }

    Some((normalized, issues))
}

fn is_latlon_terminator(token: &str) -> bool {
    matches!(token.trim(), "$$" | "&&")
}

fn split_packed_coordinate_pair(normalized: &[String], token: &str) -> Option<(String, String)> {
    if token.starts_with('-') || token.len() != 8 || !normalized.len().is_multiple_of(2) {
        return None;
    }

    let lat = token[..4].to_string();
    let lon = token[4..].to_string();
    parse_coordinate(&lat, true)?;
    parse_coordinate(&lon, false)?;

    Some((lat, lon))
}

fn consume_coordinate_token(tokens: &[&str], index: &mut usize) -> Option<String> {
    let mut token = String::new();

    while *index < tokens.len() {
        token.push_str(tokens[*index].trim());
        *index += 1;

        if (4..=8).contains(&token.len()) {
            if !token
                .trim_start_matches('-')
                .chars()
                .all(|character| character.is_ascii_digit())
            {
                return None;
            }
            return Some(token);
        }
        if token.len() > 8 {
            return None;
        }
    }

    None
}

fn parse_coordinate(coord: &str, is_latitude: bool) -> Option<f64> {
    let coord = coord.trim();

    if coord.starts_with('-') {
        return coord.parse().ok();
    }

    let value = match coord.len() {
        4 | 5 => {
            let hundredths: f64 = coord.parse().ok()?;
            hundredths / 100.0
        }
        6 => {
            let degrees: f64 = coord[0..2].parse().ok()?;
            let minutes: f64 = coord[2..4].parse().ok()?;
            let seconds: f64 = coord[4..6].parse().ok()?;
            degrees + minutes / 60.0 + seconds / 3600.0
        }
        7 => {
            let degrees: f64 = coord[0..2].parse().ok()?;
            let decimal: f64 = coord[2..7].parse().ok()?;
            degrees + decimal / 100000.0
        }
        8 => {
            let degrees: f64 = coord[0..2].parse().ok()?;
            let minutes: f64 = coord[2..4].parse().ok()?;
            let seconds: f64 = coord[4..6].parse().ok()?;
            let hundredths: f64 = coord[6..8].parse().ok()?;
            degrees + minutes / 60.0 + (seconds + hundredths / 100.0) / 3600.0
        }
        _ => coord.parse().ok()?,
    };

    let value = if is_latitude {
        value
    } else if value < 40.0 {
        // Some NWS products publish western longitudes without the leading
        // `10x`, yielding implausibly small magnitudes like `11.23`. Preserve
        // the existing narrow recovery by shifting those values back into the
        // expected western-hemisphere range.
        value + 100.0
    } else {
        value
    };

    Some(if is_latitude { value } else { -value })
}

fn split_fused_coordinate_token(normalized: &[String], token: &str) -> Option<(String, String)> {
    if token.starts_with('-') || token.len() != 8 || normalized.len().is_multiple_of(2) {
        return None;
    }

    if normalized
        .iter()
        .any(|coord| coord.starts_with('-') || coord.len() > 5)
    {
        return None;
    }

    let lon = token[..4].to_string();
    let lat = token[4..].to_string();
    let pending_latitude = normalized.last()?;

    parse_coordinate(pending_latitude, true)?;
    parse_coordinate(&lon, false)?;
    parse_coordinate(&lat, true)?;

    Some((lon, lat))
}

fn validate_polygon(
    mut points: Vec<(f64, f64)>,
    raw: &str,
    _issues: &mut Vec<ProductParseIssue>,
) -> Result<Vec<(f64, f64)>, ProductParseIssue> {
    if points.len() < 3 {
        return Err(ProductParseIssue::new(
            "latlon_parse",
            "invalid_latlon_coordinate_count",
            format!("LAT...LON block has an invalid coordinate count: `{raw}`"),
            Some(raw.to_string()),
        ));
    }

    if points.first() != points.last() {
        points.push(points[0]);
    }

    if points.len() > 4 && has_self_intersection(&points) {
        return Err(ProductParseIssue::new(
            "latlon_parse",
            "latlon_geometry_invalid",
            format!("LAT...LON block produced a self-intersecting polygon: `{raw}`"),
            Some(raw.to_string()),
        ));
    }

    Ok(points)
}

fn has_self_intersection(points: &[(f64, f64)]) -> bool {
    let segment_count = points.len().saturating_sub(1);
    for first_index in 0..segment_count {
        let first_start = point_xy(points[first_index]);
        let first_end = point_xy(points[first_index + 1]);

        for second_index in (first_index + 1)..segment_count {
            if second_index == first_index + 1
                || (first_index == 0 && second_index == segment_count - 1)
            {
                continue;
            }

            let second_start = point_xy(points[second_index]);
            let second_end = point_xy(points[second_index + 1]);
            if segments_intersect(first_start, first_end, second_start, second_end) {
                return true;
            }
        }
    }

    false
}

fn point_xy(point: (f64, f64)) -> (f64, f64) {
    (point.1, point.0)
}

fn segments_intersect(
    first_start: (f64, f64),
    first_end: (f64, f64),
    second_start: (f64, f64),
    second_end: (f64, f64),
) -> bool {
    let first_orientation = orientation(first_start, first_end, second_start);
    let second_orientation = orientation(first_start, first_end, second_end);
    let third_orientation = orientation(second_start, second_end, first_start);
    let fourth_orientation = orientation(second_start, second_end, first_end);

    first_orientation * second_orientation < 0.0 && third_orientation * fourth_orientation < 0.0
}

fn orientation(first: (f64, f64), second: (f64, f64), third: (f64, f64)) -> f64 {
    (second.1 - first.1) * (third.0 - second.0) - (second.0 - first.0) * (third.1 - second.1)
}

fn starts_new_section_header(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty()
        && !looks_like_coordinate_line(trimmed)
        && (trimmed.contains("...")
            || trimmed.starts_with('/')
            || trimmed.split_whitespace().next().is_some_and(|token| {
                token.chars().all(|ch| ch.is_ascii_uppercase()) && token.len() >= 3
            }))
}

fn looks_like_coordinate_line(line: &str) -> bool {
    line.split_whitespace().all(|token| {
        let digits = token.trim_start_matches('-');
        !digits.is_empty() && digits.chars().all(|ch| ch.is_ascii_digit())
    })
}

fn serialize_points<S>(points: &[(f64, f64)], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut seq = serializer.serialize_seq(Some(points.len()))?;
    for (lat, lon) in points {
        seq.serialize_element(&[round_coordinate(*lat), round_coordinate(*lon)])?;
    }
    seq.end()
}

fn round_coordinate(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_polygon_4digit() {
        let text = "LAT...LON 4400 8900 4500 8800 4600 8900 4400 8900";
        let polygons = parse_latlon_polygons(text);

        assert_eq!(polygons.len(), 1);
        assert_eq!(polygons[0].points.len(), 4);
        assert!((polygons[0].points[0].0 - 44.0).abs() < 0.01);
        assert!((polygons[0].points[0].1 + 89.0).abs() < 0.01);
        assert!((polygons[0].points[1].0 - 45.0).abs() < 0.01);
        assert!((polygons[0].points[1].1 + 88.0).abs() < 0.01);
    }

    #[test]
    fn parse_polygon_4digit_hundredths() {
        let text = "LAT...LON 4430 8930 4530 8830 4430 8930";
        let polygons = parse_latlon_polygons(text);

        assert_eq!(polygons.len(), 1);
        assert!((polygons[0].points[0].0 - 44.30).abs() < 0.01);
        assert!((polygons[0].points[0].1 + 89.30).abs() < 0.01);
    }

    #[test]
    fn parse_polygon_6digit() {
        let text = "LAT...LON 443012 893056 443015 893100 443012 893056";
        let polygons = parse_latlon_polygons(text);

        assert_eq!(polygons.len(), 1);
        let expected_lat = 44.0 + 30.0 / 60.0 + 12.0 / 3600.0;
        assert!((polygons[0].points[0].0 - expected_lat).abs() < 0.0001);
    }

    #[test]
    fn parse_polygon_8digit() {
        let text = "LAT...LON 44308930 45308830 44308930";
        let polygons = parse_latlon_polygons(text);

        assert_eq!(polygons.len(), 1);
        assert!((polygons[0].points[0].0 - 44.30).abs() < 0.00001);
        assert!((polygons[0].points[0].1 + 89.30).abs() < 0.00001);
    }

    #[test]
    fn serializes_points_without_float_artifacts() {
        let text = "LAT...LON 4000 9802 4000 9822 4011 9817";
        let polygons = parse_latlon_polygons(text);

        let json = serde_json::to_value(&polygons[0]).expect("polygon serializes");
        assert_eq!(
            json["points"],
            serde_json::json!([
                [40.0, -98.02],
                [40.0, -98.22],
                [40.11, -98.17],
                [40.0, -98.02]
            ])
        );
    }

    #[test]
    fn parse_latlon_invalid_skipped() {
        let polygons = parse_latlon_polygons("LAT...LON INVALID");
        assert!(polygons.is_empty());
    }

    #[test]
    fn parse_latlon_invalid_reports_issue() {
        let (polygons, issues) = parse_latlon_polygons_with_issues("LAT...LON INVALID");
        assert!(polygons.is_empty());
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "invalid_latlon_coordinate_format");
    }

    #[test]
    fn parse_multiple_polygons() {
        let text =
            "LAT...LON 4000 9800 4100 9800 4000 9800\nLAT...LON 4200 9900 4300 9900 4200 9900";
        let polygons = parse_latlon_polygons(text);

        assert_eq!(polygons.len(), 2);
    }

    #[test]
    fn parse_latlon_case_insensitive() {
        let text = "lat...lon 4000 9800 4100 9800 4000 9800";
        let polygons = parse_latlon_polygons(text);

        assert_eq!(polygons.len(), 1);
    }

    #[test]
    fn parse_latlon_wrapped_across_lines() {
        let text = "LAT...LON 4000 9800 4100 9800\n4200 9900 4000 9800";
        let polygons = parse_latlon_polygons(text);

        assert_eq!(polygons.len(), 1);
        assert_eq!(polygons[0].points.len(), 4);
    }

    #[test]
    fn parse_latlon_with_split_coordinate_token() {
        let text = "LAT...LON 4000 9800 4100 98\n00 4000 9800";
        let polygons = parse_latlon_polygons(text);

        assert_eq!(polygons.len(), 1);
        assert_eq!(polygons[0].points.len(), 3);
    }

    #[test]
    fn parse_latlon_repairs_fused_middle_token() {
        let text = "LAT...LON 4000 9800 4100 98112979 4000 9800";
        let (_polygons, issues) = parse_latlon_polygons_with_issues(text);

        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "latlon_fused_token_repaired")
        );
    }

    #[test]
    fn parse_latlon_with_packed_wrapped_tail() {
        let text = "LAT...LON 44308930 4530\n8830 44308930";
        let polygons = parse_latlon_polygons(text);

        assert_eq!(polygons.len(), 1);
    }

    #[test]
    fn parse_latlon_packed_pair_token_emits_no_repair_issue() {
        let text = "LAT...LON 40009800 41009800 42009900";
        let (_polygons, issues) = parse_latlon_polygons_with_issues(text);

        assert!(issues.is_empty());
    }

    #[test]
    fn polygon_auto_closes() {
        let text = "LAT...LON 4000 9800 4100 9800 4200 9900";
        let polygons = parse_latlon_polygons(text);

        assert_eq!(polygons[0].points.first(), polygons[0].points.last());
    }

    #[test]
    fn parse_latlon_rejects_self_intersecting_polygon() {
        let text = "LAT...LON 4000 9800 4200 9900 4000 9900 4200 9800";
        let (polygons, issues) = parse_latlon_polygons_with_issues(text);

        assert!(polygons.is_empty());
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "latlon_geometry_invalid")
        );
    }

    #[test]
    fn parse_latlon_odd_coordinates() {
        let text = "LAT...LON 4000 9800 4200";
        let (polygons, issues) = parse_latlon_polygons_with_issues(text);

        assert!(polygons.is_empty());
        assert_eq!(issues[0].code, "invalid_latlon_coordinate_count");
    }

    #[test]
    fn parse_latlon_svsgrrmi_polygon() {
        let text = "LAT...LON 4257 8531 4254 8520 4235 8521 4231 8529 4236 8538 4252 8540";
        let polygons = parse_latlon_polygons(text);

        assert_eq!(polygons.len(), 1);
    }

    #[test]
    fn parse_candidate_terminated_by_unrelated_section_header() {
        let text = "LAT...LON 4000 9800 4100 9800\nTIME...MOT...LOC 2310Z 238DEG 39KT 3221 08853";
        let (polygons, issues) = parse_latlon_polygons_with_issues(text);

        assert!(polygons.is_empty());
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "invalid_latlon_coordinate_count")
        );
    }

    #[test]
    fn parse_multiple_latlon_blocks_in_multiline_product() {
        let text = "LAT...LON 4000 9800 4100 9800 4000 9800\nOTHER TEXT\nlat...lon 4200 9900 4300 9900 4200 9900";
        let polygons = parse_latlon_polygons(text);

        assert_eq!(polygons.len(), 2);
    }

    #[test]
    fn parse_packed_wrapped_and_fused_tokens_in_same_block() {
        let text = "LAT...LON 4000 9800 4100 98112979\n42009800";
        let (_polygons, issues) = parse_latlon_polygons_with_issues(text);

        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "latlon_fused_token_repaired")
        );
    }

    #[test]
    fn polygon_wkt_output_is_stable() {
        let text = "LAT...LON 4000 9800 4100 9800 4200 9900";
        let polygons = parse_latlon_polygons(text);

        assert_eq!(
            polygons[0].wkt,
            "POLYGON((-98.000000 40.000000, -98.000000 41.000000, -99.000000 42.000000, -98.000000 40.000000))"
        );
    }

    #[test]
    fn parse_latlon_ignores_inline_dollar_terminator() {
        let text = "LAT...LON 4076 7784 4078 7792 4084 7797 $$";
        let polygons = parse_latlon_polygons(text);

        assert_eq!(polygons.len(), 1);
    }

    #[test]
    fn parse_latlon_ignores_inline_ampersand_terminator() {
        let text = "LAT...LON 4076 7784 4078 7792 4084 7797 &&";
        let polygons = parse_latlon_polygons(text);

        assert_eq!(polygons.len(), 1);
    }
}
