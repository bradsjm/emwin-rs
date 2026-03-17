//! Parsing for Local Storm Report bulletins.

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use regex::Regex;
use serde::Serialize;
use std::sync::OnceLock;

use crate::ProductParseIssue;

/// Structured LSR bulletin.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LsrBulletin {
    pub reports: Vec<LsrReport>,
    pub is_summary: bool,
}

/// One LSR report row.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LsrReport {
    pub valid: String,
    pub event_text: String,
    pub city: String,
    pub county: Option<String>,
    pub state: Option<String>,
    pub latitude: f64,
    pub longitude: f64,
    pub source: Option<String>,
    pub remark: Option<String>,
    pub magnitude_value: Option<f64>,
    pub magnitude_units: Option<String>,
    pub magnitude_qualifier: Option<String>,
}

pub(crate) fn parse_lsr_bulletin(
    text: &str,
    reference_time: DateTime<Utc>,
) -> Option<(LsrBulletin, Vec<ProductParseIssue>)> {
    let normalized = text.replace('\r', "");
    let mut reports = Vec::new();
    let mut issues = Vec::new();
    let chunks = split_lsr_chunks(&normalized);
    for chunk in &chunks {
        match parse_lsr_chunk(chunk, reference_time) {
            Some(report) => reports.push(report),
            None => issues.push(ProductParseIssue::new(
                "lsr_parse",
                "invalid_lsr_report",
                "could not parse LSR report block",
                Some(chunk.trim().to_string()),
            )),
        }
    }

    (!reports.is_empty()).then_some((
        LsrBulletin {
            reports,
            is_summary: normalized.to_ascii_uppercase().contains("...SUMMARY"),
        },
        issues,
    ))
}

fn split_lsr_chunks(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut start = None;
    let mut chunks = Vec::new();
    for idx in 0..lines.len() {
        if is_time_line(lines[idx])
            && let Some(existing) = start.replace(idx)
        {
            chunks.push(lines[existing..idx].join("\n"));
        }
    }
    if let Some(existing) = start {
        chunks.push(lines[existing..].join("\n"));
    }
    chunks
}

fn is_time_line(line: &str) -> bool {
    let trimmed = line.trim_end();
    trimmed.len() >= 29
        && trimmed
            .get(..4)
            .is_some_and(|prefix| prefix.chars().all(|c| c.is_ascii_digit()))
        && trimmed
            .get(4..)
            .is_some_and(|suffix| suffix.starts_with(' '))
}

fn parse_lsr_chunk(text: &str, reference_time: DateTime<Utc>) -> Option<LsrReport> {
    let mut lines = text.lines();
    let first = lines.next()?.trim_end();
    let second = lines.next()?.trim_end();
    let remarks = lines
        .take_while(|line| !line.trim().starts_with("&&") && !line.trim().starts_with("$$"))
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    let first_fields = parse_first_lsr_line(first)?;
    let second_fields = parse_second_lsr_line(second)?;
    let valid = parse_lsr_datetime(
        &first_fields.time_token,
        &first_fields.ampm,
        &second_fields.date,
        reference_time,
    )?;
    let (latitude, longitude) = parse_lat_lon(&first_fields.lat_token, &first_fields.lon_token)?;
    let (magnitude_value, magnitude_units, magnitude_qualifier) =
        parse_magnitude(&second_fields.magnitude);

    Some(LsrReport {
        valid: valid.to_rfc3339(),
        event_text: first_fields.event_text,
        city: first_fields.city,
        county: empty_to_none(second_fields.county.trim()),
        state: empty_to_none(second_fields.state.trim()),
        latitude,
        longitude,
        source: empty_to_none(second_fields.source.trim()),
        remark: empty_to_none(remarks.trim()),
        magnitude_value,
        magnitude_units,
        magnitude_qualifier,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedLsrFirstLine {
    time_token: String,
    ampm: String,
    event_text: String,
    city: String,
    lat_token: String,
    lon_token: String,
}

fn parse_first_lsr_line(line: &str) -> Option<ParsedLsrFirstLine> {
    let time_token = line.get(..4)?;
    if !time_token
        .chars()
        .all(|character| character.is_ascii_digit())
    {
        return None;
    }

    let mut cursor = 4;
    while line
        .as_bytes()
        .get(cursor)
        .is_some_and(u8::is_ascii_whitespace)
    {
        cursor += 1;
    }

    let ampm = match line.get(cursor..cursor + 2)? {
        "AM" | "PM" => line.get(cursor..cursor + 2)?.to_string(),
        _ => return None,
    };
    cursor += 2;

    while line
        .as_bytes()
        .get(cursor)
        .is_some_and(u8::is_ascii_whitespace)
    {
        cursor += 1;
    }

    let payload = line.get(cursor..)?;
    let mut tail = payload.rsplitn(3, char::is_whitespace);
    let lon_token = tail.next()?.trim().to_string();
    let lat_token = tail.next()?.trim().to_string();
    let middle = tail.next()?.trim_end();

    if !is_lsr_lat_token(&lat_token) || !is_lsr_lon_token(&lon_token) {
        return None;
    }

    let (event_text, city) = split_first_line_middle(middle)?;
    if event_text.is_empty() || city.is_empty() {
        return None;
    }

    Some(ParsedLsrFirstLine {
        time_token: time_token.to_string(),
        ampm,
        event_text,
        city,
        lat_token,
        lon_token,
    })
}

fn split_first_line_middle(middle: &str) -> Option<(String, String)> {
    let field_split = middle
        .char_indices()
        .nth(17)
        .map(|(index, _)| index)
        .unwrap_or(middle.len());
    if field_split < middle.len() {
        let event_text = middle.get(..field_split)?.trim().to_string();
        let city = middle.get(field_split..)?.trim().to_string();
        if !event_text.is_empty() && !city.is_empty() {
            return Some((event_text, city));
        }
    }

    let split_at = middle.find("  ")?;
    let event_text = middle.get(..split_at)?.trim().to_string();
    let city = middle.get(split_at..)?.trim().to_string();
    (!event_text.is_empty() && !city.is_empty()).then_some((event_text, city))
}

fn is_lsr_lat_token(token: &str) -> bool {
    token.len() >= 2
        && token.ends_with(['N', 'S'])
        && token
            .get(..token.len() - 1)
            .unwrap_or_default()
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.')
}

fn is_lsr_lon_token(token: &str) -> bool {
    token.len() >= 2
        && token.ends_with(['E', 'W'])
        && token
            .get(..token.len() - 1)
            .unwrap_or_default()
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.')
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedLsrSecondLine {
    date: String,
    magnitude: String,
    county: String,
    state: String,
    source: String,
}

fn parse_second_lsr_line(line: &str) -> Option<ParsedLsrSecondLine> {
    let captures = second_line_re().captures(line)?;
    let date = captures.name("date")?.as_str().trim();
    let magnitude = captures
        .name("mag")
        .map(|value| value.as_str().trim())
        .unwrap_or_default();
    let county = captures.name("county")?.as_str().trim();
    let state = captures.name("state")?.as_str().trim();
    let source = captures.name("source")?.as_str().trim();

    Some(ParsedLsrSecondLine {
        date: date.to_string(),
        magnitude: magnitude.to_string(),
        county: county.to_string(),
        state: state.to_string(),
        source: source.to_string(),
    })
}

fn second_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^(?P<date>\d{2}/\d{2}/\d{4})\s+(?P<mag>.{0,17}?)\s{2,}(?P<county>.*?)\s+(?P<state>[A-Z]{2})\s{2,}(?P<source>.+?)\s*$",
        )
        .expect("valid LSR second-line regex")
    })
}

fn parse_lsr_datetime(
    hhmm: &str,
    ampm: &str,
    date: &str,
    _reference_time: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let naive =
        NaiveDateTime::parse_from_str(&format!("{date} {hhmm} {ampm}"), "%m/%d/%Y %I%M %p").ok()?;
    Some(Utc.from_utc_datetime(&naive))
}

fn parse_lat_lon(lat: &str, lon: &str) -> Option<(f64, f64)> {
    let latitude = lat.get(..lat.len().checked_sub(1)?)?.parse::<f64>().ok()?;
    let lat = if lat.ends_with('S') {
        -latitude
    } else {
        latitude
    };
    let longitude = lon.get(..lon.len().checked_sub(1)?)?.parse::<f64>().ok()?;
    let lon = if lon.ends_with('E') {
        longitude
    } else {
        -longitude
    };
    Some((lat, lon))
}

fn magnitude_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^(?:(?P<qual>[EMULTG><=]+)\s*)?(?P<value>-?\d+(?:\.\d+)?)\s*(?P<units>[A-Za-z/]+)?$",
        )
        .expect("valid LSR magnitude regex")
    })
}

fn parse_magnitude(raw: &str) -> (Option<f64>, Option<String>, Option<String>) {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (None, None, None);
    }
    let Some(caps) = magnitude_re().captures(trimmed) else {
        return (None, None, None);
    };
    let value = caps
        .name("value")
        .and_then(|m| m.as_str().parse::<f64>().ok());
    let units = value.and_then(|_| {
        caps.name("units")
            .map(|m| m.as_str().trim().to_ascii_uppercase())
    });
    let qualifier = caps.name("qual").map(|m| m.as_str().trim().to_string());
    (value, units, qualifier)
}

fn empty_to_none(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::parse_lsr_bulletin;
    use chrono::Utc;

    #[test]
    fn parses_local_lsr_report() {
        let text = "\
0150 AM     HAIL             BROOKSVILLE             34.40N 87.70W
03/10/2026  1.00 IN          WINSTON             AL  PUBLIC
QUARTER SIZE HAIL REPORTED
&&";
        let (bulletin, issues) = parse_lsr_bulletin(text, Utc::now()).expect("lsr bulletin");
        assert_eq!(bulletin.reports.len(), 1);
        assert!(issues.is_empty());
        assert_eq!(bulletin.reports[0].city, "BROOKSVILLE");
        assert_eq!(bulletin.reports[0].state.as_deref(), Some("AL"));
    }

    #[test]
    fn malformed_lsr_block_reports_issue_but_keeps_valid_report() {
        let text = "\
0150 AM     HAIL             BROOKSVILLE             34.40N 87.70W
03/10/2026  1.00 IN          WINSTON             AL  PUBLIC
0145 AM     HAIL             NOWHERE                 34.00N 87.00W
03/10/2026
&&";
        let (bulletin, issues) = parse_lsr_bulletin(text, Utc::now()).expect("lsr bulletin");
        assert_eq!(bulletin.reports.len(), 1);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "invalid_lsr_report");
    }

    #[test]
    fn parses_city_field_that_begins_with_digit() {
        let text = "\
0236 PM     Non-Tstm Wnd Gst 7 NW Elk Mountain       41.75N 106.51W
03/11/2026  M63 MPH          Carbon             WY   Mesonet

            WYDOT Sensor at Halleck Ridge.

&&";
        let (bulletin, issues) = parse_lsr_bulletin(text, Utc::now()).expect("lsr bulletin");

        assert_eq!(bulletin.reports.len(), 1);
        assert_eq!(bulletin.reports[0].event_text, "Non-Tstm Wnd Gst");
        assert_eq!(bulletin.reports[0].city, "7 NW Elk Mountain");
        assert_eq!(bulletin.reports[0].magnitude_value, Some(63.0));
        assert_eq!(bulletin.reports[0].magnitude_units.as_deref(), Some("MPH"));
        assert_eq!(
            bulletin.reports[0].magnitude_qualifier.as_deref(),
            Some("M")
        );
        assert!(issues.is_empty());
    }

    #[test]
    fn parses_mixed_case_units_without_losing_value_or_qualifier() {
        let text = "\
1130 AM     Snow             2 E Escanaba            45.74N 87.05W
03/13/2026  M9.0 Inch        Delta              MI   Trained Spotter

            Snowfall through 11:30 A.M. in Escanaba.

&&";
        let (bulletin, issues) = parse_lsr_bulletin(text, Utc::now()).expect("lsr bulletin");

        assert_eq!(bulletin.reports.len(), 1);
        assert_eq!(bulletin.reports[0].magnitude_value, Some(9.0));
        assert_eq!(bulletin.reports[0].magnitude_units.as_deref(), Some("INCH"));
        assert_eq!(
            bulletin.reports[0].magnitude_qualifier.as_deref(),
            Some("M")
        );
        assert!(issues.is_empty());
    }
}
