//! NWS TIME...MOT...LOC (Time Motion Location) parsing module.
//!
//! TIME...MOT...LOC lines provide storm movement and location information in
//! severe weather products. They indicate when a storm was observed, its
//! direction and speed of movement, and its geographic location.

use chrono::{DateTime, Utc};
use serde::ser::{SerializeSeq, Serializer};

use crate::ProductParseIssue;
use crate::body::support::{ascii_find_case_insensitive, format_linestring_wkt};
use crate::time::resolve_clock_time_nearest;

const MARKER: &str = "TIME...MOT...LOC";

/// Parsed TIME...MOT...LOC entry containing storm movement and position data.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TimeMotLocEntry {
    /// UTC time resolved to a full timestamp.
    pub time_utc: DateTime<Utc>,
    /// Motion direction in degrees.
    pub direction_degrees: u16,
    /// Motion speed in knots.
    pub speed_kt: u16,
    /// Coordinate pairs as (latitude, longitude) in decimal degrees.
    #[serde(serialize_with = "serialize_points")]
    pub points: Vec<(f64, f64)>,
    /// WKT representation as POINT or LINESTRING.
    pub wkt: String,
}

impl TimeMotLocEntry {
    pub(crate) fn from_parts(
        time_utc: DateTime<Utc>,
        direction_degrees: u16,
        speed_kt: u16,
        points: Vec<(f64, f64)>,
    ) -> Self {
        Self {
            time_utc,
            direction_degrees,
            speed_kt,
            wkt: format_linestring_wkt(&points),
            points,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TimeMotLocCandidate<'a> {
    raw: String,
    source_line: &'a str,
    marker_was_embedded: bool,
}

/// Parses all TIME...MOT...LOC entries found in the given text.
pub fn parse_time_mot_loc_entries(
    text: &str,
    reference_time: DateTime<Utc>,
) -> Vec<TimeMotLocEntry> {
    parse_time_mot_loc_entries_with_issues(text, reference_time).0
}

/// Parses TIME...MOT...LOC entries and returns any parsing issues encountered.
pub fn parse_time_mot_loc_entries_with_issues(
    text: &str,
    reference_time: DateTime<Utc>,
) -> (Vec<TimeMotLocEntry>, Vec<ProductParseIssue>) {
    let mut entries = Vec::new();
    let mut issues = Vec::new();
    let marker_count = count_markers(text);
    let candidates = find_time_mot_loc_candidates(text);

    for candidate in &candidates {
        if candidate.marker_was_embedded {
            // Some upstream products run the marker into prior content instead
            // of starting a new line. Warn and continue from the marker.
            issues.push(ProductParseIssue::new(
                "time_mot_loc_parse",
                "time_mot_loc_poorly_formatted",
                "TIME...MOT...LOC marker was not line-aligned in the source text",
                Some(candidate.source_line.to_string()),
            ));
        }

        match parse_time_mot_loc_candidate(candidate, reference_time) {
            Ok((entry, parse_issues)) => {
                entries.push(entry);
                issues.extend(parse_issues);
            }
            Err(issue) => issues.push(issue),
        }
    }

    if marker_count > candidates.len() {
        issues.push(ProductParseIssue::new(
            "time_mot_loc_parse",
            "time_mot_loc_regex_failed_after_marker",
            "found TIME...MOT...LOC marker but could not match the expected field layout",
            None,
        ));
    }

    (entries, issues)
}

fn count_markers(text: &str) -> usize {
    text.lines()
        .map(|line| {
            let mut count = 0;
            let mut search = line;
            while let Some(position) = ascii_find_case_insensitive(search, MARKER) {
                count += 1;
                search = &search[position + MARKER.len()..];
            }
            count
        })
        .sum()
}

fn find_time_mot_loc_candidates(text: &str) -> Vec<TimeMotLocCandidate<'_>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut candidates = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        let Some(marker_position) = ascii_find_case_insensitive(line, MARKER) else {
            index += 1;
            continue;
        };

        let candidate = collect_time_mot_loc_candidate(&lines, index, marker_position);
        index += candidate.1.max(1);
        if let Some(raw) = candidate.0 {
            candidates.push(TimeMotLocCandidate {
                raw,
                source_line: line,
                marker_was_embedded: marker_position > 0,
            });
        }
    }

    candidates
}

fn collect_time_mot_loc_candidate(
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

        if candidate_has_parseable_prefix(&raw) && !has_odd_coordinate_tail(&raw) {
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

    if candidate_has_parseable_prefix(&raw) {
        (Some(raw), consumed)
    } else {
        (None, consumed)
    }
}

fn candidate_has_parseable_prefix(raw: &str) -> bool {
    let mut tokens = raw.split_whitespace();
    match (
        tokens.next(),
        tokens.next(),
        parse_degrees_token(&mut tokens),
        parse_speed_token(&mut tokens),
    ) {
        (Some(marker), Some(time), Some(_), Some(_)) => {
            marker.eq_ignore_ascii_case(MARKER) && parse_time_shape(time)
        }
        _ => false,
    }
}

fn has_odd_coordinate_tail(raw: &str) -> bool {
    let mut tokens = raw.split_whitespace();
    let _ = tokens.next();
    let _ = tokens.next();
    let _ = parse_degrees_token(&mut tokens);
    let _ = parse_speed_token(&mut tokens);
    let coord_count = tokens.count();
    coord_count > 0 && !coord_count.is_multiple_of(2)
}

fn parse_time_mot_loc_candidate(
    candidate: &TimeMotLocCandidate<'_>,
    reference_time: DateTime<Utc>,
) -> Result<(TimeMotLocEntry, Vec<ProductParseIssue>), ProductParseIssue> {
    let raw = candidate.raw.trim();
    let mut tokens = raw.split_whitespace();

    let marker = tokens.next().ok_or_else(|| invalid_format_issue(raw))?;
    if !marker.eq_ignore_ascii_case(MARKER) {
        return Err(invalid_format_issue(raw));
    }

    let time_token = tokens.next().ok_or_else(|| invalid_format_issue(raw))?;
    let time_utc = parse_time_token(time_token, reference_time).ok_or_else(|| {
        ProductParseIssue::new(
            "time_mot_loc_parse",
            "invalid_time_mot_loc_time",
            format!("could not parse TIME...MOT...LOC time from line: `{raw}`"),
            Some(raw.to_string()),
        )
    })?;

    let direction_degrees =
        parse_degrees_token(&mut tokens).ok_or_else(|| invalid_direction_issue(raw))?;
    let speed_kt = parse_speed_token(&mut tokens).ok_or_else(|| invalid_speed_issue(raw))?;

    let (mut coord_tokens, mut issues) =
        consume_coordinate_tokens(tokens).ok_or_else(|| invalid_coordinate_format_issue(raw))?;
    if coord_tokens.len() % 2 != 0 {
        let dropped = coord_tokens
            .pop()
            .expect("odd coordinate count has trailing token");
        // Some source products lose the final coordinate half through wrapping
        // or transmission damage. Preserve the complete pairs and surface the
        // loss instead of discarding the whole candidate.
        issues.push(ProductParseIssue::new(
            "time_mot_loc_parse",
            "time_mot_loc_truncated_dangling_coordinate",
            format!(
                "dropped dangling TIME...MOT...LOC coordinate token `{dropped}` after source formatting loss"
            ),
            Some(raw.to_string()),
        ));
    }
    if coord_tokens.len() < 2 {
        return Err(ProductParseIssue::new(
            "time_mot_loc_parse",
            "invalid_time_mot_loc_coordinate_count",
            format!("TIME...MOT...LOC line has an invalid coordinate count: `{raw}`"),
            Some(raw.to_string()),
        ));
    }

    let mut points = Vec::with_capacity(coord_tokens.len() / 2);
    for pair in coord_tokens.chunks(2) {
        let lat = parse_coordinate(&pair[0], true).ok_or_else(|| {
            ProductParseIssue::new(
                "time_mot_loc_parse",
                "invalid_time_mot_loc_latitude",
                format!("could not parse TIME...MOT...LOC latitude from line: `{raw}`"),
                Some(raw.to_string()),
            )
        })?;
        let lon = parse_coordinate(&pair[1], false).ok_or_else(|| {
            ProductParseIssue::new(
                "time_mot_loc_parse",
                "invalid_time_mot_loc_longitude",
                format!("could not parse TIME...MOT...LOC longitude from line: `{raw}`"),
                Some(raw.to_string()),
            )
        })?;
        points.push((lat, lon));
    }

    Ok((
        TimeMotLocEntry {
            time_utc,
            direction_degrees,
            speed_kt,
            wkt: format_linestring_wkt(&points),
            points,
        },
        issues,
    ))
}

fn parse_time_shape(token: &str) -> bool {
    token.len() == 5 && token.ends_with('Z') && token[..4].chars().all(|ch| ch.is_ascii_digit())
}

fn parse_time_token(token: &str, reference_time: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if !parse_time_shape(token) {
        return None;
    }

    let hour: u32 = token[0..2].parse().ok()?;
    let minute: u32 = token[2..4].parse().ok()?;
    resolve_clock_time_nearest(reference_time, hour, minute)
}

fn parse_degrees_token<'a, I>(tokens: &mut I) -> Option<u16>
where
    I: Iterator<Item = &'a str>,
{
    let token = tokens.next()?;
    if let Some(value) = token.strip_suffix("DEG") {
        return value.parse().ok();
    }
    if token.chars().all(|ch| ch.is_ascii_digit()) {
        let unit = tokens.next()?;
        if unit.eq_ignore_ascii_case("DEG") {
            return token.parse().ok();
        }
    }
    None
}

fn parse_speed_token<'a, I>(tokens: &mut I) -> Option<u16>
where
    I: Iterator<Item = &'a str>,
{
    let token = tokens.next()?;
    if let Some(value) = token.strip_suffix("KT") {
        return value.parse().ok();
    }
    if token.chars().all(|ch| ch.is_ascii_digit()) {
        let unit = tokens.next()?;
        if unit.eq_ignore_ascii_case("KT") {
            return token.parse().ok();
        }
    }
    None
}

fn consume_coordinate_tokens<'a, I>(tokens: I) -> Option<(Vec<String>, Vec<ProductParseIssue>)>
where
    I: Iterator<Item = &'a str>,
{
    let raw_coord_tokens: Vec<&str> = tokens.collect();
    normalize_coordinate_tokens(&raw_coord_tokens)
}

fn normalize_coordinate_tokens(tokens: &[&str]) -> Option<(Vec<String>, Vec<ProductParseIssue>)> {
    let mut normalized = Vec::new();
    let issues = Vec::new();
    let mut index = 0;

    while index < tokens.len() {
        // Wrapped source lines sometimes split a single coordinate across
        // whitespace boundaries, e.g. `088` + `53` instead of `08853`.
        let token = consume_coordinate_token(tokens, &mut index)?;
        if let Some((lat, lon)) = split_packed_coordinate_pair(&normalized, &token) {
            normalized.push(lat);
            normalized.push(lon);
            continue;
        }
        normalized.push(token);
    }

    Some((normalized, issues))
}

fn split_packed_coordinate_pair(normalized: &[String], token: &str) -> Option<(String, String)> {
    if token.len() != 8 || !normalized.len().is_multiple_of(2) {
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
            return Some(token);
        }
        if token.len() > 8 {
            return None;
        }
    }

    None
}

fn invalid_format_issue(raw: &str) -> ProductParseIssue {
    ProductParseIssue::new(
        "time_mot_loc_parse",
        "invalid_time_mot_loc_format",
        format!("could not parse TIME...MOT...LOC line: `{raw}`"),
        Some(raw.to_string()),
    )
}

fn invalid_direction_issue(raw: &str) -> ProductParseIssue {
    ProductParseIssue::new(
        "time_mot_loc_parse",
        "invalid_time_mot_loc_direction",
        format!("could not parse TIME...MOT...LOC direction from line: `{raw}`"),
        Some(raw.to_string()),
    )
}

fn invalid_speed_issue(raw: &str) -> ProductParseIssue {
    ProductParseIssue::new(
        "time_mot_loc_parse",
        "invalid_time_mot_loc_speed",
        format!("could not parse TIME...MOT...LOC speed from line: `{raw}`"),
        Some(raw.to_string()),
    )
}

fn invalid_coordinate_format_issue(raw: &str) -> ProductParseIssue {
    ProductParseIssue::new(
        "time_mot_loc_parse",
        "invalid_time_mot_loc_coordinate_format",
        format!("could not normalize TIME...MOT...LOC coordinates from line: `{raw}`"),
        Some(raw.to_string()),
    )
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
    line.split_whitespace()
        .all(|token| !token.is_empty() && token.chars().all(|ch| ch.is_ascii_digit()))
}

fn parse_coordinate(text: &str, is_lat: bool) -> Option<f64> {
    let digits = text.trim().trim_start_matches('-');
    let negative = !is_lat;

    let value = match digits.len() {
        4 => {
            let degrees: f64 = digits[0..2].parse().ok()?;
            let hundredths: f64 = digits[2..4].parse().ok()?;
            degrees + (hundredths / 100.0)
        }
        5 => {
            let degrees_digits = if is_lat { 2 } else { 3 };
            let degrees: f64 = digits[0..degrees_digits].parse().ok()?;
            let hundredths: f64 = digits[degrees_digits..].parse().ok()?;
            degrees + (hundredths / 100.0)
        }
        6 => {
            let degrees_digits = if is_lat { 2 } else { 3 };
            let minutes_digits = digits.len() - degrees_digits;

            let degrees: f64 = digits[0..degrees_digits].parse().ok()?;
            let minutes: f64 = digits[degrees_digits..degrees_digits + minutes_digits]
                .parse()
                .ok()?;
            degrees + (minutes / 60.0)
        }
        8 => {
            let degrees_digits = if is_lat { 2 } else { 3 };
            let whole_minutes_digits = 2;
            let hundredths_digits = digits.len() - degrees_digits - whole_minutes_digits;

            let degrees: f64 = digits[0..degrees_digits].parse().ok()?;
            let whole_minutes: f64 = digits[degrees_digits..degrees_digits + whole_minutes_digits]
                .parse()
                .ok()?;
            let hundredths: f64 = digits[degrees_digits + whole_minutes_digits
                ..degrees_digits + whole_minutes_digits + hundredths_digits]
                .parse()
                .ok()?;
            degrees + ((whole_minutes + (hundredths / 100.0)) / 60.0)
        }
        _ => return None,
    };

    Some(if negative { -value } else { value })
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

    fn reference_time() -> DateTime<Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-03-06T23:15:00Z")
            .expect("reference time parses")
            .with_timezone(&Utc)
    }

    #[test]
    fn parse_time_mot_loc_point() {
        let text = "TIME...MOT...LOC 2310Z 238DEG 39KT 3221 08853";
        let entries = parse_time_mot_loc_entries(text, reference_time());

        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].time_utc.to_rfc3339(),
            "2026-03-06T23:10:00+00:00"
        );
        assert_eq!(entries[0].direction_degrees, 238);
        assert_eq!(entries[0].speed_kt, 39);
        assert_eq!(entries[0].points.len(), 1);
        assert!(entries[0].wkt.starts_with("POINT("));
    }

    #[test]
    fn parse_time_mot_loc_linestring() {
        let text = "TIME...MOT...LOC 2359Z 332DEG 25KT 3704 9736 3699 9720";
        let reference_time = chrono::DateTime::parse_from_rfc3339("2026-03-06T23:55:00Z")
            .expect("reference time parses")
            .with_timezone(&Utc);
        let entries = parse_time_mot_loc_entries(text, reference_time);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].points.len(), 2);
        assert!(entries[0].wkt.starts_with("LINESTRING("));
    }

    #[test]
    fn parse_time_mot_loc_invalid_reports_issue() {
        let text = "TIME...MOT...LOC 2359Z 332DEG 25KT 3704";
        let reference_time = chrono::DateTime::parse_from_rfc3339("2026-03-06T23:55:00Z")
            .expect("reference time parses")
            .with_timezone(&Utc);
        let (entries, issues) = parse_time_mot_loc_entries_with_issues(text, reference_time);

        assert!(entries.is_empty());
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "invalid_time_mot_loc_coordinate_count");
    }

    #[test]
    fn parse_time_mot_loc_wrapped_across_lines() {
        let text = "TIME...MOT...LOC 2310Z 238DEG 39KT\r\n3221 08853 3225 08849\n";
        let entries = parse_time_mot_loc_entries(text, reference_time());

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].points.len(), 2);
    }

    #[test]
    fn parse_time_mot_loc_with_split_coordinate_token() {
        let text = "TIME...MOT...LOC 2310Z 238DEG 39KT 3221 088\r\n53 3225 08849\n";
        let entries = parse_time_mot_loc_entries(text, reference_time());

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].points.len(), 2);
        assert!((entries[0].points[0].1 + 88.53).abs() < 0.01);
    }

    #[test]
    fn parse_time_mot_loc_with_packed_coordinate_pair_token() {
        let text = "TIME...MOT...LOC 2310Z 238DEG 39KT 32210853 32250849\n";
        let (entries, issues) = parse_time_mot_loc_entries_with_issues(text, reference_time());

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].points.len(), 2);
        assert!(issues.is_empty());
    }

    #[test]
    fn serializes_points_without_float_artifacts() {
        let text = "TIME...MOT...LOC 0009Z 212DEG 40KT 4017 09764";
        let reference_time = chrono::DateTime::parse_from_rfc3339("2026-03-07T00:10:00Z")
            .expect("reference time parses")
            .with_timezone(&Utc);
        let entries = parse_time_mot_loc_entries(text, reference_time);

        let json = serde_json::to_value(&entries[0]).expect("entry serializes");
        assert_eq!(json["points"], serde_json::json!([[40.17, -97.64]]));
    }

    #[test]
    fn parse_time_mot_loc_rolls_to_previous_day_when_closest() {
        let text = "TIME...MOT...LOC 2359Z 332DEG 25KT 3704 9736";
        let reference_time = chrono::DateTime::parse_from_rfc3339("2026-03-07T00:05:00Z")
            .expect("reference time parses")
            .with_timezone(&Utc);
        let entries = parse_time_mot_loc_entries(text, reference_time);

        assert_eq!(
            entries[0].time_utc.to_rfc3339(),
            "2026-03-06T23:59:00+00:00"
        );
    }

    #[test]
    fn parse_time_mot_loc_reports_dangling_coordinate_repair() {
        let text = "TIME...MOT...LOC 2310Z 238DEG 39KT 3221 08853 3225";
        let (entries, issues) = parse_time_mot_loc_entries_with_issues(text, reference_time());

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].points.len(), 1);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "time_mot_loc_truncated_dangling_coordinate");
    }

    #[test]
    fn parse_time_mot_loc_reports_non_line_aligned_marker() {
        let text = "BULLETIN TIME...MOT...LOC 2310Z 238DEG 39KT 3221 08853";
        let (entries, issues) = parse_time_mot_loc_entries_with_issues(text, reference_time());

        assert_eq!(entries.len(), 1);
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "time_mot_loc_poorly_formatted")
        );
    }

    #[test]
    fn parse_time_mot_loc_marker_with_mixed_case_parses() {
        let text = "Time...Mot...Loc 2310Z 238DEG 39KT 3221 08853";
        let entries = parse_time_mot_loc_entries(text, reference_time());

        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn parse_time_mot_loc_with_trailing_unrelated_text_stops_cleanly() {
        let text = "TIME...MOT...LOC 2310Z 238DEG 39KT 3221 08853\nHAILTHREAT...RADARINDICATED";
        let entries = parse_time_mot_loc_entries(text, reference_time());

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].points.len(), 1);
    }

    #[test]
    fn parse_time_mot_loc_coordinates_spanning_many_lines_parse() {
        let text = "TIME...MOT...LOC 2310Z 238DEG 39KT\n3221 08853\n3225 08849\n3230 08844\n";
        let entries = parse_time_mot_loc_entries(text, reference_time());

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].points.len(), 3);
    }

    #[test]
    fn parse_time_mot_loc_invalid_motion_tokens_report_invalid_format() {
        for text in [
            "TIME...MOT...LOC 2310Z 238XYZ 39KT 3221 08853",
            "TIME...MOT...LOC 2310Z 238DEG 39MPH 3221 08853",
        ] {
            let (entries, issues) = parse_time_mot_loc_entries_with_issues(text, reference_time());

            assert!(entries.is_empty(), "text={text}");
            assert!(
                issues
                    .iter()
                    .any(|issue| issue.code == "time_mot_loc_regex_failed_after_marker"),
                "text={text}"
            );
        }
    }
}
