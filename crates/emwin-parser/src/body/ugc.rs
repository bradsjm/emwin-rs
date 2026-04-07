//! NWS UGC (Universal Geographic Code) parsing module.
//!
//! UGC sections are line-oriented blocks with dense shorthand expansion. This
//! parser preserves that behavior while avoiding the previous full-text line
//! collection and regex-heavy candidate assembly.

use crate::ProductParseIssue;
use crate::data::{ugc_county_entry, ugc_zone_entry};
use crate::time::resolve_day_time_not_before;
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;

/// A parsed UGC section containing codes and expiration time.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct UgcSection {
    /// County UGC areas grouped by state/prefix.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub counties: BTreeMap<String, Vec<UgcArea>>,
    /// Zone UGC areas grouped by state/prefix.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub zones: BTreeMap<String, Vec<UgcArea>>,
    /// Fire weather UGC areas grouped by state/prefix.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub fire_zones: BTreeMap<String, Vec<UgcArea>>,
    /// Marine UGC areas grouped by state/prefix.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub marine_zones: BTreeMap<String, Vec<UgcArea>>,
    /// Expiration time for this UGC section.
    pub expires: DateTime<Utc>,
}

/// Enriched county or zone area within a UGC section.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct UgcArea {
    /// Three-digit UGC identifier within the enclosing state.
    pub id: u16,
    /// Human-readable county or zone name when present in the generated lookup catalog.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<&'static str>,
    /// Representative latitude for the county or zone when present in the generated lookup catalog.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    /// Representative longitude for the county or zone when present in the generated lookup catalog.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
}

/// A single UGC code representing a county or zone.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct UgcCode {
    /// 2-letter state code (e.g., "IA", "NE")
    pub state: String,
    /// Geographic class (County or Zone)
    pub geoclass: UgcClass,
    /// 3-digit county/zone number
    pub number: u16,
}

/// Geographic classification for UGC codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum UgcClass {
    /// County (C)
    County,
    /// Zone (Z)
    Zone,
    /// Fire Zone (F)
    FireZone,
    /// Marine Zone (M)
    Marine,
    /// Unknown classification
    Unknown,
}

impl UgcClass {
    pub(crate) fn from_char(c: char) -> Self {
        match c {
            'C' => UgcClass::County,
            'Z' => UgcClass::Zone,
            'F' => UgcClass::FireZone,
            'M' => UgcClass::Marine,
            _ => UgcClass::Unknown,
        }
    }

    pub(crate) fn as_char(self) -> char {
        match self {
            UgcClass::County => 'C',
            UgcClass::Zone => 'Z',
            UgcClass::FireZone => 'F',
            UgcClass::Marine => 'M',
            UgcClass::Unknown => 'Z',
        }
    }
}

impl UgcSection {
    pub(crate) fn from_codes(codes: Vec<UgcCode>, expires: DateTime<Utc>) -> Self {
        let mut counties = BTreeMap::new();
        let mut zones = BTreeMap::new();
        let mut fire_zones = BTreeMap::new();
        let mut marine_zones = BTreeMap::new();

        for code in codes {
            match code.geoclass {
                UgcClass::County => {
                    counties
                        .entry(code.state.clone())
                        .or_insert_with(Vec::new)
                        .push(build_area(&code, Some(ugc_county_entry)));
                }
                UgcClass::Zone | UgcClass::Unknown => {
                    let bucket = if is_marine_zone_state(&code.state) {
                        &mut marine_zones
                    } else {
                        &mut zones
                    };
                    bucket
                        .entry(code.state.clone())
                        .or_insert_with(Vec::new)
                        .push(build_area(&code, Some(ugc_zone_entry)));
                }
                UgcClass::FireZone => {
                    let area = build_area(&code, None);
                    fire_zones
                        .entry(code.state)
                        .or_insert_with(Vec::new)
                        .push(area);
                }
                UgcClass::Marine => {
                    let area = build_area(&code, None);
                    marine_zones
                        .entry(code.state)
                        .or_insert_with(Vec::new)
                        .push(area);
                }
            }
        }

        Self {
            counties,
            zones,
            fire_zones,
            marine_zones,
            expires,
        }
    }
}

fn build_area(
    code: &UgcCode,
    lookup: Option<fn(&str) -> Option<&'static crate::data::UgcLocationEntry>>,
) -> UgcArea {
    let canonical = format!(
        "{}{}{:03}",
        code.state,
        code.geoclass.as_char(),
        code.number
    );
    let entry = lookup.and_then(|lookup| lookup(&canonical));

    UgcArea {
        id: code.number,
        name: entry.map(|entry| entry.name),
        lat: entry.map(|entry| entry.latitude),
        lon: entry.map(|entry| entry.longitude),
    }
}

fn is_marine_zone_state(state: &str) -> bool {
    matches!(
        state,
        "AM" | "AN"
            | "GM"
            | "LC"
            | "LE"
            | "LH"
            | "LM"
            | "LO"
            | "LS"
            | "PH"
            | "PK"
            | "PM"
            | "PS"
            | "PZ"
            | "SL"
    )
}

/// Parses all UGC sections found in the given text.
pub fn parse_ugc_sections(text: &str, valid_time: DateTime<Utc>) -> Vec<UgcSection> {
    parse_ugc_sections_with_issues(text, valid_time).0
}

/// Parses all UGC sections found in the given text and returns any issues.
pub fn parse_ugc_sections_with_issues(
    text: &str,
    valid_time: DateTime<Utc>,
) -> (Vec<UgcSection>, Vec<ProductParseIssue>) {
    let mut sections = Vec::new();
    let mut issues = Vec::new();

    for candidate in extract_ugc_candidates(text) {
        match parse_ugc_capture(&candidate, valid_time) {
            Ok(section) => sections.push(section),
            Err(issue) => issues.push(issue),
        }
    }

    (sections, issues)
}

fn extract_ugc_candidates(text: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut lines = iter_candidate_lines(text).peekable();

    while let Some(line) = lines.next() {
        if !is_ugc_start(line) {
            continue;
        }

        if let Some(candidate) = collect_ugc_candidate(line, &mut lines) {
            candidates.push(candidate);
        }
    }

    candidates
}

fn iter_candidate_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines().map(|line| line.trim_end_matches('\r'))
}

fn collect_ugc_candidate<'a, I>(
    first_line: &'a str,
    remaining_lines: &mut std::iter::Peekable<I>,
) -> Option<String>
where
    I: Iterator<Item = &'a str>,
{
    let mut combined = compact_ugc_text(first_line);

    while !has_ugc_expiration_suffix(&combined) {
        let Some(next_line) = remaining_lines.peek().copied() else {
            break;
        };
        let next = next_line.trim();
        if next.is_empty() || starts_new_section_header(next) {
            break;
        }
        combined.push_str(&compact_ugc_text(next));
        let _ = remaining_lines.next();
    }

    has_ugc_expiration_suffix(&combined).then_some(combined)
}

fn compact_ugc_text(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
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
    if !is_prefix_match {
        return false;
    }

    trimmed
        .as_bytes()
        .get(6)
        .is_none_or(|character| matches!(character, b'-' | b'>' | b',' | b' ' | b'\t'))
}

fn has_ugc_expiration_suffix(text: &str) -> bool {
    text.len() >= 8
        && text.ends_with('-')
        && text[text.len() - 7..text.len() - 1]
            .chars()
            .all(|character| character.is_ascii_digit())
        && text.as_bytes()[text.len() - 8] == b'-'
}

fn starts_new_section_header(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }

    if trimmed.ends_with(':') || trimmed.contains("...") {
        return true;
    }

    let first_token = trimmed.split_whitespace().next().unwrap_or("");
    !is_ugc_start(trimmed)
        && first_token.len() >= 4
        && first_token
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_uppercase())
        && first_token.chars().all(|character| {
            character.is_ascii_uppercase()
                || character.is_ascii_digit()
                || matches!(character, '/' | '-')
        })
}

fn extract_expiration(text: &str) -> Option<(String, String)> {
    if !has_ugc_expiration_suffix(text) {
        return None;
    }

    let split_at = text.len() - 8;
    let code_block = &text[..split_at];
    let expire_code = &text[split_at + 1..text.len() - 1];
    Some((code_block.to_string(), expire_code.to_string()))
}

fn parse_ugc_capture(
    text: &str,
    valid_time: DateTime<Utc>,
) -> Result<UgcSection, ProductParseIssue> {
    let (code_block, expire_code) = extract_expiration(text).ok_or_else(|| {
        ProductParseIssue::new(
            "ugc_parse",
            "invalid_ugc_format",
            format!("could not parse UGC section: `{text}`"),
            Some(text.to_string()),
        )
    })?;

    let codes = expand_ugc_block(&code_block).ok_or_else(|| {
        ProductParseIssue::new(
            "ugc_parse",
            "invalid_ugc_codes",
            format!("could not parse UGC codes from section: `{text}`"),
            Some(text.to_string()),
        )
    })?;
    let expires = parse_expire_time(&expire_code, valid_time).ok_or_else(|| {
        ProductParseIssue::new(
            "ugc_parse",
            "invalid_ugc_expiration",
            format!("could not parse UGC expiration from section: `{text}`"),
            Some(text.to_string()),
        )
    })?;

    Ok(UgcSection::from_codes(codes, expires))
}

fn expand_ugc_block(block: &str) -> Option<Vec<UgcCode>> {
    let mut codes = Vec::new();
    let mut current_prefix: Option<(String, UgcClass)> = None;

    for segment in block.split([',', '-']) {
        if segment.is_empty() {
            continue;
        }

        let parsed = parse_ugc_segment(segment.trim(), &mut current_prefix)?;
        codes.extend(parsed);
    }

    if codes.is_empty() { None } else { Some(codes) }
}

/// Expands both full (`IAC001>003`) and shorthand (`004>007`) segments.
///
/// The dense expansion is intentional and matches the long-standing UGC
/// contract exposed by the crate.
fn parse_ugc_segment(
    segment: &str,
    current_prefix: &mut Option<(String, UgcClass)>,
) -> Option<Vec<UgcCode>> {
    let (state, geoclass, numeric) =
        if let Some((state, geoclass, numeric)) = parse_full_ugc_segment(segment) {
            *current_prefix = Some((state.clone(), geoclass));
            (state, geoclass, numeric)
        } else if is_ugc_numeric_sequence(segment) {
            let (state, geoclass) = current_prefix.clone()?;
            (state, geoclass, segment)
        } else {
            return None;
        };

    let mut codes = Vec::new();
    for (start_num, end_num) in parse_ugc_numeric_ranges(numeric)? {
        for number in start_num..=end_num {
            codes.push(UgcCode {
                state: state.clone(),
                geoclass,
                number,
            });
        }
    }

    Some(codes)
}

fn parse_full_ugc_segment(segment: &str) -> Option<(String, UgcClass, &str)> {
    if segment.len() < 6 {
        return None;
    }

    let state = &segment[..2];
    let geoclass = segment.as_bytes().get(2).copied()? as char;
    if !state
        .chars()
        .all(|character| character.is_ascii_uppercase())
    {
        return None;
    }
    if !matches!(geoclass, 'C' | 'Z' | 'F' | 'M') {
        return None;
    }

    let numeric = &segment[3..];
    if is_ugc_numeric_sequence(numeric) {
        Some((state.to_string(), UgcClass::from_char(geoclass), numeric))
    } else {
        None
    }
}

fn is_ugc_numeric_sequence(segment: &str) -> bool {
    let tokens = segment.split('>').collect::<Vec<_>>();
    let Some(first) = tokens.first().copied() else {
        return false;
    };

    if !is_three_digit_ugc_token(first) {
        return false;
    }

    if tokens.len() == 1 {
        return true;
    }

    if tokens.len() == 2 {
        return is_one_to_three_digit_ugc_token(tokens[1]);
    }

    tokens.len().is_multiple_of(2)
        && tokens[1..]
            .iter()
            .copied()
            .all(is_one_to_three_digit_ugc_token)
}

fn parse_ugc_numeric_ranges(numeric: &str) -> Option<Vec<(u16, u16)>> {
    let tokens = numeric.split('>').collect::<Vec<_>>();
    let first = parse_ugc_number(tokens.first().copied()?)?;

    if tokens.len() == 1 {
        return Some(vec![(first, first)]);
    }

    if tokens.len() == 2 {
        let end = parse_ugc_range_end(first, tokens[1])?;
        return Some(vec![(first, end)]);
    }

    if !tokens.len().is_multiple_of(2) {
        return None;
    }

    let mut ranges = Vec::with_capacity(tokens.len() / 2);
    let mut index = 0;
    while index + 1 < tokens.len() {
        let start = parse_ugc_number(tokens[index])?;
        let end = parse_ugc_range_end(start, tokens[index + 1])?;
        ranges.push((start, end));
        index += 2;
    }

    Some(ranges)
}

fn parse_ugc_range_end(start: u16, token: &str) -> Option<u16> {
    if !is_one_to_three_digit_ugc_token(token) {
        return None;
    }

    if token.len() == 3 {
        let end = parse_ugc_number(token)?;
        return (end >= start).then_some(end);
    }

    let suffix: u16 = token.parse().ok()?;
    let magnitude = 10u16.checked_pow(token.len() as u32)?;
    let prefix = (start / magnitude) * magnitude;
    let end = prefix + suffix;
    (end >= start).then_some(end)
}

fn is_three_digit_ugc_token(token: &str) -> bool {
    token.len() == 3 && token.chars().all(|character| character.is_ascii_digit())
}

fn is_one_to_three_digit_ugc_token(token: &str) -> bool {
    (1..=3).contains(&token.len()) && token.chars().all(|character| character.is_ascii_digit())
}

fn parse_ugc_number(text: &str) -> Option<u16> {
    if !is_three_digit_ugc_token(text) {
        return None;
    }
    text.parse().ok()
}

fn parse_expire_time(expire_code: &str, valid_time: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if expire_code.len() != 6 {
        return None;
    }

    let day: u32 = expire_code[0..2].parse().ok()?;
    let hour: u32 = expire_code[2..4].parse().ok()?;
    let minute: u32 = expire_code[4..6].parse().ok()?;

    resolve_day_time_not_before(valid_time, day, hour, minute)
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeZone, Utc};

    use super::{UgcArea, parse_ugc_sections, parse_ugc_sections_with_issues};

    fn ids(areas: &[UgcArea]) -> Vec<u16> {
        areas.iter().map(|area| area.id).collect()
    }

    fn names(areas: &[UgcArea]) -> Vec<Option<&'static str>> {
        areas.iter().map(|area| area.name).collect()
    }

    fn test_valid_time() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2025, 3, 5, 12, 0, 0).unwrap()
    }

    #[test]
    fn parse_single_ugc() {
        let text = "IAC001-051200-\n";
        let sections = parse_ugc_sections(text, test_valid_time());

        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].counties["IA"][0].id, 1);
        assert_eq!(sections[0].counties["IA"][0].name, Some("Adair"));
    }

    #[test]
    fn parse_ugc_range() {
        let text = "IAC001>003-051200-\n";
        let sections = parse_ugc_sections(text, test_valid_time());

        assert_eq!(sections.len(), 1);
        assert_eq!(ids(&sections[0].counties["IA"]), vec![1, 2, 3]);
        assert_eq!(
            names(&sections[0].counties["IA"]),
            vec![Some("Adair"), None, Some("Adams")]
        );
    }

    #[test]
    fn parse_ugc_abbreviated_range_end() {
        let text = "IDZ051>75-121100-\n";
        let sections = parse_ugc_sections(text, test_valid_time());

        assert_eq!(sections.len(), 1);
        assert_eq!(
            sections[0].zones["ID"]
                .iter()
                .map(|area| area.id)
                .collect::<Vec<_>>(),
            (51..=75).collect::<Vec<_>>()
        );
    }

    #[test]
    fn parse_ugc_multiple_range_pairs_in_single_segment() {
        let text = "MDZ501>502>509>510-121100-\n";
        let sections = parse_ugc_sections(text, test_valid_time());

        assert_eq!(sections.len(), 1);
        assert_eq!(
            sections[0].zones["MD"]
                .iter()
                .map(|area| area.id)
                .collect::<Vec<_>>(),
            vec![501, 502, 509, 510]
        );
    }

    #[test]
    fn parse_ugc_combines_abbreviated_and_standard_ranges() {
        let text = "MTZ028>42-056>058-112100-\n";
        let sections = parse_ugc_sections(text, test_valid_time());

        assert_eq!(sections.len(), 1);
        let ids = sections[0].zones["MT"]
            .iter()
            .map(|area| area.id)
            .collect::<Vec<_>>();
        assert!(ids.starts_with(&(28..=42).collect::<Vec<_>>()));
        assert!(ids.ends_with(&[56, 57, 58]));
    }

    #[test]
    fn parse_pyiem_nm_county_sample_keeps_dense_county_expansion() {
        let text = "NMC001-005>007-011-019-027-028-031-033-039-041-043-045-047-049-053-055-057-061-040300-\n";
        let sections = parse_ugc_sections(text, test_valid_time());

        assert_eq!(sections.len(), 1);
        assert_eq!(
            ids(&sections[0].counties["NM"]),
            vec![
                1, 5, 6, 7, 11, 19, 27, 28, 31, 33, 39, 41, 43, 45, 47, 49, 53, 55, 57, 61
            ]
        );
        assert!(
            sections[0].counties["NM"]
                .iter()
                .any(|area| area.id == 6 && area.name == Some("Cibola"))
        );
    }

    #[test]
    fn parse_ugc_multiple() {
        let text = "IAC001-IAC003-IAC005-051200-\n";
        let sections = parse_ugc_sections(text, test_valid_time());

        assert_eq!(sections.len(), 1);
        assert_eq!(ids(&sections[0].counties["IA"]), vec![1, 3, 5]);
    }

    #[test]
    fn candidate_spanning_more_than_eight_lines_is_supported() {
        let text = "\
IAC001-\n\
003-\n\
005-\n\
007-\n\
009-\n\
011-\n\
013-\n\
015-\n\
017-\n\
019-\n\
051200-\n";
        let sections = parse_ugc_sections(text, test_valid_time());

        assert_eq!(sections.len(), 1);
        assert_eq!(
            ids(&sections[0].counties["IA"]),
            vec![1, 3, 5, 7, 9, 11, 13, 15, 17, 19]
        );
    }

    #[test]
    fn block_terminated_by_blank_line_is_ignored() {
        let text = "IAC001-\n\nNEXT SECTION:\n";
        let (sections, issues) = parse_ugc_sections_with_issues(text, test_valid_time());

        assert!(sections.is_empty());
        assert!(issues.is_empty());
    }

    #[test]
    fn block_terminated_by_next_section_after_expiration_still_parses() {
        let text = "IAC001-051200-\nLAT...LON 4123 09312\n";
        let sections = parse_ugc_sections(text, test_valid_time());

        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].counties["IA"][0].id, 1);
    }

    #[test]
    fn mixed_comma_and_hyphen_shorthand_segments_parse() {
        let text = "IAC001,003-005>007-051200-\n";
        let sections = parse_ugc_sections(text, test_valid_time());

        assert_eq!(sections.len(), 1);
        assert_eq!(ids(&sections[0].counties["IA"]), vec![1, 3, 5, 6, 7]);
    }

    #[test]
    fn invalid_expiration_fragment_is_ignored_without_full_block() {
        let text = "IAC001-\nBROKEN CONTINUATION\n";
        let (sections, issues) = parse_ugc_sections_with_issues(text, test_valid_time());

        assert!(sections.is_empty());
        assert!(issues.is_empty());
    }

    #[test]
    fn dotted_advisory_lines_do_not_start_ugc_capture() {
        let text = "AKZ321.WinterWeatherAdvisoryfrom4AMto4PMAKDTWednesdayforAKZ323.";
        let (sections, issues) = parse_ugc_sections_with_issues(text, test_valid_time());

        assert!(sections.is_empty());
        assert!(issues.is_empty());
    }

    #[test]
    fn incomplete_ugc_candidates_without_expiration_are_ignored() {
        let text = "OVC002\nWAZ040-10630-\n";
        let (sections, issues) = parse_ugc_sections_with_issues(text, test_valid_time());

        assert!(sections.is_empty());
        assert!(issues.is_empty());
    }
}
