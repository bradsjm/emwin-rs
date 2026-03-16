//! NWS WIND/HAIL tag parsing module.
//!
//! This module supports both legacy combined `WIND... HAIL...` tags and modern
//! line-oriented severe threat tags without relying on regex for the modern
//! parsing path.

use regex::Regex;
use std::sync::OnceLock;

use crate::ProductParseIssue;

/// Type of wind/hail tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindHailKind {
    LegacyWind,
    LegacyHail,
    WindThreat,
    MaxWindGust,
    HailThreat,
    MaxHailSize,
}

/// Parsed wind/hail tag entry with threat information.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct WindHailEntry {
    pub kind: WindHailKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numeric_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub units: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comparison: Option<char>,
}

impl WindHailEntry {
    pub(crate) fn new(
        kind: WindHailKind,
        numeric_value: Option<f64>,
        units: Option<String>,
        comparison: Option<char>,
    ) -> Self {
        Self {
            kind,
            numeric_value,
            units,
            comparison,
        }
    }
}

/// Parses all wind and hail tags found in the given text.
pub fn parse_wind_hail_entries(text: &str) -> Vec<WindHailEntry> {
    parse_wind_hail_entries_with_issues(text).0
}

/// Parses wind and hail tags and returns any parsing issues encountered.
pub fn parse_wind_hail_entries_with_issues(
    text: &str,
) -> (Vec<WindHailEntry>, Vec<ProductParseIssue>) {
    let mut entries = Vec::new();
    let mut issues = Vec::new();

    let (mut legacy_entries, mut legacy_issues) = parse_legacy_wind_hail(text);
    entries.append(&mut legacy_entries);
    issues.append(&mut legacy_issues);

    for raw_line in text.lines() {
        match parse_modern_wind_hail_line(raw_line.trim()) {
            Ok(Some(entry)) => entries.push(entry),
            Ok(None) => {}
            Err(issue) => issues.push(issue),
        }
    }

    (entries, issues)
}

/// The legacy pair is structurally stable and clearer to keep as one regex.
fn legacy_wind_hail_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?is)WIND\.\.\.\s*([<>])?\s*([0-9]+)\s*(MPH|KTS)\s+HAIL\.\.\.\s*([<>])?\s*([0-9.]+)\s*IN",
        )
        .expect("legacy wind hail regex compiles")
    })
}

fn parse_legacy_wind_hail(text: &str) -> (Vec<WindHailEntry>, Vec<ProductParseIssue>) {
    let mut entries = Vec::new();
    let mut issues = Vec::new();

    for captures in legacy_wind_hail_regex().captures_iter(text) {
        let Some(raw) = captures.get(0).map(|value| value.as_str()) else {
            continue;
        };

        let wind_value = captures
            .get(2)
            .and_then(|value| value.as_str().parse::<f64>().ok());
        let wind_units = captures
            .get(3)
            .map(|value| value.as_str().to_ascii_uppercase());
        let hail_value = captures
            .get(5)
            .and_then(|value| value.as_str().parse::<f64>().ok());

        match (wind_value, wind_units, hail_value) {
            (Some(wind_value), Some(wind_units), Some(hail_value)) => {
                entries.push(WindHailEntry {
                    kind: WindHailKind::LegacyWind,
                    numeric_value: Some(wind_value),
                    units: Some(wind_units),
                    comparison: captures
                        .get(1)
                        .and_then(|value| value.as_str().chars().next()),
                });
                entries.push(WindHailEntry {
                    kind: WindHailKind::LegacyHail,
                    numeric_value: Some(hail_value),
                    units: Some("IN".to_string()),
                    comparison: captures
                        .get(4)
                        .and_then(|value| value.as_str().chars().next()),
                });
            }
            _ => issues.push(invalid_wind_hail_issue(raw)),
        }
    }

    (entries, issues)
}

/// Parses one modern wind/hail line by splitting on the literal `...`.
fn parse_modern_wind_hail_line(line: &str) -> Result<Option<WindHailEntry>, ProductParseIssue> {
    if line.is_empty() || !line.contains("...") {
        return Ok(None);
    }

    let Some((label, value)) = line.split_once("...") else {
        return Err(invalid_wind_hail_issue(line));
    };

    let label = normalize_modern_label(label);
    let value = value.trim();

    let entry = match label.as_str() {
        "HAILTHREAT" => WindHailEntry {
            kind: WindHailKind::HailThreat,
            numeric_value: None,
            units: None,
            comparison: None,
        },
        "WINDTHREAT" => WindHailEntry {
            kind: WindHailKind::WindThreat,
            numeric_value: None,
            units: None,
            comparison: None,
        },
        "HAIL" | "MAXHAILSIZE" => parse_numeric_value(
            value,
            WindHailKind::MaxHailSize,
            &["IN"],
            "invalid_wind_hail_hail_value",
            line,
        )?,
        "WIND" | "MAXWINDGUST" => parse_numeric_value(
            value,
            WindHailKind::MaxWindGust,
            &["MPH", "KTS"],
            "invalid_wind_hail_wind_value",
            line,
        )?,
        _ => return Ok(None),
    };

    Ok(Some(entry))
}

fn normalize_modern_label(label: &str) -> String {
    label
        .chars()
        .filter(|character| !character.is_whitespace())
        .map(|character| character.to_ascii_uppercase())
        .collect()
}

fn parse_numeric_value(
    value: &str,
    kind: WindHailKind,
    allowed_units: &[&str],
    error_code: &'static str,
    raw: &str,
) -> Result<WindHailEntry, ProductParseIssue> {
    let normalized = normalize_modern_value(value);
    let value = normalized.as_str();
    let (comparison, remainder) = match value.chars().next() {
        Some('<') | Some('>') => (value.chars().next(), value[1..].trim()),
        _ => (None, value),
    };

    let (numeric_str, units) = split_numeric_and_units(remainder, allowed_units)
        .ok_or_else(|| invalid_numeric_issue(error_code, raw))?;

    let numeric_value = numeric_str
        .parse::<f64>()
        .map_err(|_| invalid_numeric_issue(error_code, raw))?;

    Ok(WindHailEntry {
        kind,
        numeric_value: Some(numeric_value),
        units: Some(units.to_string()),
        comparison,
    })
}

fn normalize_modern_value(value: &str) -> String {
    let mut normalized = value.trim().to_string();

    loop {
        let stripped = normalized
            .strip_suffix("$$")
            .or_else(|| normalized.strip_suffix("&&"));
        let Some(stripped) = stripped else {
            break;
        };
        normalized = stripped.trim_end().to_string();
    }

    normalized
        .trim_end_matches(['.', ',', ';', ':'])
        .trim_end()
        .to_string()
}

fn split_numeric_and_units<'a>(
    value: &'a str,
    allowed_units: &[&'a str],
) -> Option<(&'a str, &'a str)> {
    let mut parts = value.split_whitespace();
    let first = parts.next()?;

    if let Some(second) = parts.next() {
        if parts.next().is_some() {
            return None;
        }
        let units = second.to_ascii_uppercase();
        return allowed_units
            .iter()
            .copied()
            .find(|allowed| *allowed == units)
            .map(|allowed| (first, allowed));
    }

    let split_index = first
        .find(|character: char| character.is_ascii_alphabetic())
        .filter(|index| *index > 0)?;
    let numeric = &first[..split_index];
    let units = first[split_index..].to_ascii_uppercase();
    allowed_units
        .iter()
        .copied()
        .find(|allowed| *allowed == units)
        .map(|allowed| (numeric, allowed))
}

fn invalid_numeric_issue(code: &'static str, raw: &str) -> ProductParseIssue {
    ProductParseIssue::new(
        "wind_hail_parse",
        code,
        format!("could not parse wind/hail value from line: `{raw}`"),
        Some(raw.to_string()),
    )
}

fn invalid_wind_hail_issue(raw: &str) -> ProductParseIssue {
    ProductParseIssue::new(
        "wind_hail_parse",
        "invalid_wind_hail_format",
        format!("could not parse wind/hail line: `{raw}`"),
        Some(raw.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_legacy_wind_hail_tags() {
        let text = "WIND...>60MPH HAIL...<1.00IN";
        let entries = parse_wind_hail_entries(text);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind, WindHailKind::LegacyWind);
        assert_eq!(entries[0].comparison, Some('>'));
        assert_eq!(entries[1].kind, WindHailKind::LegacyHail);
        assert_eq!(entries[1].comparison, Some('<'));
    }

    #[test]
    fn parse_modern_wind_hail_tags() {
        let text = "HAILTHREAT...RADARINDICATED\nMAXHAILSIZE...1.00 IN\nWINDTHREAT...OBSERVED\nMAXWINDGUST...60 MPH";
        let entries = parse_wind_hail_entries(text);

        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].kind, WindHailKind::HailThreat);
        assert_eq!(entries[1].kind, WindHailKind::MaxHailSize);
        assert_eq!(entries[2].kind, WindHailKind::WindThreat);
        assert_eq!(entries[3].kind, WindHailKind::MaxWindGust);
    }

    #[test]
    fn parse_invalid_modern_wind_hail_reports_issue() {
        let text = "MAXWINDGUST...FAST";
        let (entries, issues) = parse_wind_hail_entries_with_issues(text);

        assert!(entries.is_empty());
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "invalid_wind_hail_wind_value");
    }

    #[test]
    fn parse_modern_hail_with_leading_dot_decimal() {
        let text = "MAX HAIL SIZE...<.75 IN";
        let entries = parse_wind_hail_entries(text);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, WindHailKind::MaxHailSize);
        assert_eq!(entries[0].comparison, Some('<'));
        assert_eq!(entries[0].numeric_value, Some(0.75));
        assert_eq!(entries[0].units.as_deref(), Some("IN"));
    }

    #[test]
    fn lowercase_modern_labels_parse() {
        let text = "max wind gust...60 mph";
        let entries = parse_wind_hail_entries(text);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, WindHailKind::MaxWindGust);
    }

    #[test]
    fn spaced_modern_labels_parse() {
        let text = "MAX   HAIL  SIZE...1.50 IN";
        let entries = parse_wind_hail_entries(text);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, WindHailKind::MaxHailSize);
        assert_eq!(entries[0].numeric_value, Some(1.5));
    }

    #[test]
    fn legacy_embedded_in_longer_sentence_still_parses() {
        let text = "EXPECT WIND...>60MPH HAIL...<1.00IN WITH THIS STORM";
        let entries = parse_wind_hail_entries(text);

        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn mixed_modern_and_legacy_tags_parse() {
        let text = "WIND...>60MPH HAIL...<1.00IN\nMAX HAIL SIZE...1.25 IN";
        let entries = parse_wind_hail_entries(text);

        assert_eq!(entries.len(), 3);
        assert!(
            entries
                .iter()
                .any(|entry| entry.kind == WindHailKind::LegacyWind)
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry.kind == WindHailKind::MaxHailSize)
        );
    }

    #[test]
    fn invalid_units_keep_existing_issue_code() {
        let text = "MAX WIND GUST...60 MPS";
        let (_entries, issues) = parse_wind_hail_entries_with_issues(text);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "invalid_wind_hail_wind_value");
    }

    #[test]
    fn compact_hail_value_parses() {
        let text = "HAIL...0.00IN";
        let entries = parse_wind_hail_entries(text);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, WindHailKind::MaxHailSize);
        assert_eq!(entries[0].numeric_value, Some(0.0));
        assert_eq!(entries[0].units.as_deref(), Some("IN"));
    }

    #[test]
    fn compact_wind_value_parses() {
        let text = "WIND...>34KTS";
        let entries = parse_wind_hail_entries(text);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, WindHailKind::MaxWindGust);
        assert_eq!(entries[0].comparison, Some('>'));
        assert_eq!(entries[0].numeric_value, Some(34.0));
        assert_eq!(entries[0].units.as_deref(), Some("KTS"));
    }

    #[test]
    fn trailing_bulletin_terminator_is_ignored() {
        let text = "MAX WIND GUST...60 MPH$$";
        let entries = parse_wind_hail_entries(text);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, WindHailKind::MaxWindGust);
        assert_eq!(entries[0].numeric_value, Some(60.0));
        assert_eq!(entries[0].units.as_deref(), Some("MPH"));
    }
}
