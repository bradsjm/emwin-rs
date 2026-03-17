//! Structured PIREP bulletin parsing.

use regex::Regex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::GeoPoint;

/// Type of PIREP report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PirepKind {
    /// Routine pilot report
    Ua,
    /// Urgent pilot report
    Uua,
}

/// Individual PIREP report from a pilot.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PirepReport {
    /// Type of report (UA or UUA)
    #[serde(rename = "kind")]
    pub report_kind: PirepKind,
    /// Reporting station/airport
    #[serde(skip_serializing_if = "Option::is_none")]
    pub station: Option<String>,
    /// Report time in HHMM format
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    /// Raw location text from /OV
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_raw: Option<String>,
    /// Resolved /OV location when supported
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<GeoPoint>,
    /// Flight level in feet
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flight_level_ft: Option<u32>,
    /// Aircraft type from /TP
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aircraft_type: Option<String>,
    /// Sky condition from /SK
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sky_condition: Option<String>,
    /// Turbulence from /TB
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turbulence: Option<String>,
    /// Icing from /IC
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icing: Option<String>,
    /// Temperature from /TA
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_c: Option<i16>,
    /// Remarks from /RM
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remarks: Option<String>,
    /// Unsupported but preserved fields by tag code
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unsupported_fields: BTreeMap<String, String>,
    /// Complete raw PIREP text
    pub raw: String,
}

/// PIREP bulletin containing multiple pilot reports.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PirepBulletin {
    /// Individual PIREP reports in the bulletin
    pub reports: Vec<PirepReport>,
}

#[derive(Debug, Clone, PartialEq)]
struct ParsedPirepRef<'a> {
    report_kind: PirepKind,
    station: Option<&'a str>,
    time: Option<String>,
    location_raw: Option<String>,
    location: Option<GeoPoint>,
    flight_level_ft: Option<u32>,
    aircraft_type: Option<String>,
    sky_condition: Option<String>,
    turbulence: Option<String>,
    icing: Option<String>,
    temperature_c: Option<i16>,
    remarks: Option<String>,
    unsupported_fields: BTreeMap<String, String>,
}

impl ParsedPirepRef<'_> {
    fn into_owned(self, raw: String) -> PirepReport {
        PirepReport {
            report_kind: self.report_kind,
            station: self.station.map(str::to_string),
            time: self.time,
            location_raw: self.location_raw,
            location: self.location,
            flight_level_ft: self.flight_level_ft,
            aircraft_type: self.aircraft_type,
            sky_condition: self.sky_condition,
            turbulence: self.turbulence,
            icing: self.icing,
            temperature_c: self.temperature_c,
            remarks: self.remarks,
            unsupported_fields: self.unsupported_fields,
            raw,
        }
    }
}

/// Parses a PIREP bulletin from text content.
pub(crate) fn parse_pirep_bulletin(text: &str) -> Option<PirepBulletin> {
    let reports = pirep_reports(text)
        .into_iter()
        .filter_map(|report| {
            parse_pirep_report_ref(&report)
                .or_else(|| parse_arp_pirep_report_ref(&report))
                .map(|parsed| parsed.into_owned(report.clone()))
        })
        .collect::<Vec<_>>();

    (!reports.is_empty()).then_some(PirepBulletin { reports })
}

/// Splits normalized text into individual `=`-delimited PIREP reports.
fn pirep_reports(text: &str) -> Vec<String> {
    let normalized = compact_ascii_whitespace(text);
    normalized
        .split('=')
        .map(str::trim)
        .filter(|report| !report.is_empty())
        .map(str::to_string)
        .collect()
}

fn compact_ascii_whitespace(text: &str) -> String {
    let mut compacted = String::with_capacity(text.len());
    let mut pending_space = false;

    for ch in text.chars() {
        if ch.is_ascii_control() && !ch.is_ascii_whitespace() {
            continue;
        }
        if ch.is_ascii_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !compacted.is_empty() {
            compacted.push(' ');
        }
        pending_space = false;
        compacted.push(ch);
    }

    compacted
}

/// Parses an individual normalized PIREP report.
fn parse_pirep_report_ref(report: &str) -> Option<ParsedPirepRef<'_>> {
    let (header, body) = report.split_once('/')?;
    let header = header.trim();
    let (report_kind, station) = if let Some(captures) = report_header_re().captures(header) {
        let report_kind = match captures.name("kind")?.as_str() {
            "UA" => PirepKind::Ua,
            "UUA" => PirepKind::Uua,
            _ => return None,
        };
        (
            report_kind,
            captures.name("station").map(|value| value.as_str()),
        )
    } else {
        let tokens = header.split_whitespace().collect::<Vec<_>>();
        let kind_idx = tokens
            .iter()
            .rposition(|token| matches!(*token, "UA" | "UUA"))?;
        let report_kind = match *tokens.get(kind_idx)? {
            "UA" => PirepKind::Ua,
            "UUA" => PirepKind::Uua,
            _ => return None,
        };
        let station = kind_idx
            .checked_sub(1)
            .and_then(|index| tokens.get(index).copied())
            .filter(|token| is_pirep_station(token));
        (report_kind, station)
    };
    let mut fields = parse_pirep_fields(body);

    let time = fields.remove("TM").map(|value| normalize_time(&value));
    let location_raw = fields.remove("OV");
    let location = location_raw.as_deref().and_then(parse_ov_latlon);
    let flight_level_ft = fields
        .remove("FL")
        .and_then(|value| parse_flight_level(&value));
    let aircraft_type = fields.remove("TP");
    let sky_condition = fields.remove("SK");
    let turbulence = fields.remove("TB");
    let icing = fields.remove("IC");
    let temperature_c = fields
        .remove("TA")
        .and_then(|value| parse_temperature_c(&value));
    let remarks = fields.remove("RM");
    let has_time = time.is_some();

    if station.is_none() && !has_time {
        return None;
    }

    Some(ParsedPirepRef {
        report_kind,
        station,
        time,
        location_raw,
        location,
        flight_level_ft,
        aircraft_type,
        sky_condition,
        turbulence,
        icing,
        temperature_c,
        remarks,
        unsupported_fields: fields,
    })
}

/// Parses slash-delimited PIREP fields into an owned field map.
fn parse_pirep_fields(report: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();

    for part in report.split('/') {
        let trimmed = part.trim();
        if trimmed.chars().count() < 3 {
            continue;
        }
        let Some(key) = trimmed.get(..2) else {
            continue;
        };
        if !key.chars().all(|ch| ch.is_ascii_uppercase()) {
            continue;
        }
        let Some(value) = trimmed.get(2..) else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        fields.insert(key.to_string(), value.to_string());
    }

    fields
}

fn parse_arp_pirep_report_ref(report: &str) -> Option<ParsedPirepRef<'_>> {
    let tokens = report.split_whitespace().collect::<Vec<_>>();
    if tokens.first().copied() != Some("ARP") {
        return None;
    }
    let location_raw = tokens.get(2).map(|value| value.to_string());
    let location = location_raw.as_deref().and_then(parse_ov_latlon);
    let time = tokens.get(3).map(|value| {
        value
            .chars()
            .filter(|ch| ch.is_ascii_digit())
            .take(4)
            .collect()
    });
    let flight_level_ft = tokens.get(4).and_then(|value| parse_flight_level(value));
    let temperature_c = tokens
        .get(5)
        .and_then(|value| parse_arp_temperature_c(value));

    Some(ParsedPirepRef {
        report_kind: PirepKind::Ua,
        station: None,
        time,
        location_raw,
        location,
        flight_level_ft,
        aircraft_type: None,
        sky_condition: None,
        turbulence: None,
        icing: None,
        temperature_c,
        remarks: None,
        unsupported_fields: BTreeMap::new(),
    })
}

fn normalize_time(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .take(4)
        .collect()
}

fn parse_ov_latlon(value: &str) -> Option<GeoPoint> {
    let compact = value
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect::<String>();
    let captures = ov_latlon_re().captures(&compact)?;
    let lat_degrees = captures.name("lat_deg")?.as_str().parse::<f64>().ok()?;
    let lat_minutes = captures
        .name("lat_min")
        .and_then(|value| value.as_str().parse::<f64>().ok())
        .unwrap_or(0.0);
    let lon_degrees = captures.name("lon_deg")?.as_str().parse::<f64>().ok()?;
    let lon_minutes = captures
        .name("lon_min")
        .and_then(|value| value.as_str().parse::<f64>().ok())
        .unwrap_or(0.0);
    let mut lat = lat_degrees + lat_minutes / 60.0;
    let mut lon = lon_degrees + lon_minutes / 60.0;
    if captures.name("lat_hemi")?.as_str() == "S" {
        lat *= -1.0;
    }
    if captures.name("lon_hemi")?.as_str() == "W" {
        lon *= -1.0;
    }
    Some(GeoPoint { lat, lon })
}

fn parse_flight_level(value: &str) -> Option<u32> {
    let digits = value
        .strip_prefix("FL")
        .or_else(|| value.strip_prefix('F'))
        .unwrap_or(value);
    let digits = digits
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    (digits.len() == 3)
        .then(|| digits.parse::<u32>().ok())
        .flatten()
        .map(|level| level.saturating_mul(100))
}

fn parse_temperature_c(value: &str) -> Option<i16> {
    let digits = value.trim().trim_end_matches('C');
    let negative = digits.starts_with('M') || digits.starts_with('-');
    let digits = digits.trim_start_matches('M').trim_start_matches('-');
    let number = digits
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>()
        .parse::<i16>()
        .ok()?;
    Some(if negative { -number } else { number })
}

fn parse_arp_temperature_c(value: &str) -> Option<i16> {
    let value = value.trim();
    let negative = value.starts_with("MS") || value.starts_with('M') || value.starts_with('-');
    let digits = value
        .trim_start_matches("MS")
        .trim_start_matches('M')
        .trim_start_matches('-');
    let number = digits.parse::<i16>().ok()?;
    Some(if negative { -number } else { number })
}

fn report_header_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?P<station>[A-Z0-9]{2,4})\s+(?P<kind>UA|UUA)\b")
            .expect("pirep header regex compiles")
    })
}

fn is_pirep_station(token: &str) -> bool {
    (2..=4).contains(&token.len())
        && token
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
        && !token.chars().all(|ch| ch.is_ascii_digit())
}

fn ov_latlon_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^(?P<lat_deg>\d{2})(?P<lat_min>\d{2})?(?P<lat_hemi>[NS])(?P<lon_deg>\d{3})(?P<lon_min>\d{2})?(?P<lon_hemi>[EW])$",
        )
        .expect("pirep ov latlon regex compiles")
    })
}

#[cfg(test)]
mod tests {
    use super::{PirepKind, parse_pirep_bulletin};

    #[test]
    fn parses_multiple_pireps() {
        let text = "DEN UA /OV 35 SW /TM 1925 /FL050 /TP E145=\nKGTF UUA /OV GTF209006/TM 1925/FL050/TP E145/TB LGT-MOD/IC NEG=\n";
        let bulletin = parse_pirep_bulletin(text).expect("pirep bulletin should parse");

        assert_eq!(bulletin.reports.len(), 2);
        assert_eq!(bulletin.reports[0].report_kind, PirepKind::Ua);
        assert_eq!(bulletin.reports[1].report_kind, PirepKind::Uua);
        assert_eq!(bulletin.reports[1].time.as_deref(), Some("1925"));
        assert_eq!(bulletin.reports[1].flight_level_ft, Some(5_000));
        assert_eq!(bulletin.reports[1].aircraft_type.as_deref(), Some("E145"));
        assert_eq!(bulletin.reports[1].turbulence.as_deref(), Some("LGT-MOD"));
    }

    #[test]
    fn keeps_station_relative_location_as_raw_text() {
        let text = "DEN UA /OV 35 SW DEN /TM 1925 /FL050 /TP E145 /TA M05=\n";
        let bulletin = parse_pirep_bulletin(text).expect("single pirep should parse");

        assert_eq!(bulletin.reports.len(), 1);
        assert_eq!(bulletin.reports[0].station.as_deref(), Some("DEN"));
        assert_eq!(bulletin.reports[0].temperature_c, Some(-5));
        assert_eq!(
            bulletin.reports[0].location_raw.as_deref(),
            Some("35 SW DEN")
        );
        assert!(bulletin.reports[0].location.is_none());
    }

    #[test]
    fn parses_direct_latlon_location_without_lookup() {
        let text = "DEN UA /OV 3905N10450W /TM 1925 /FL050=\n";
        let bulletin = parse_pirep_bulletin(text).expect("single pirep should parse");

        assert!(bulletin.reports[0].location.is_some());
    }

    #[test]
    fn wrapped_report_lines_are_normalized() {
        let text = "DEN UA /OV 35 SW DEN\n/TM 1925 /FL050\n/TP E145 /RM SMOOTH=\n";
        let bulletin = parse_pirep_bulletin(text).expect("wrapped pirep should parse");

        assert_eq!(bulletin.reports[0].time.as_deref(), Some("1925"));
        assert_eq!(bulletin.reports[0].aircraft_type.as_deref(), Some("E145"));
        assert_eq!(bulletin.reports[0].remarks.as_deref(), Some("SMOOTH"));
    }

    #[test]
    fn invalid_header_is_rejected() {
        assert!(parse_pirep_bulletin("UA /OV 35 SW=").is_none());
    }

    #[test]
    fn rejects_non_pirep_text() {
        assert!(parse_pirep_bulletin("AREA FORECAST DISCUSSION").is_none());
    }

    #[test]
    fn parses_arp_style_wmo_pirep_fixture() {
        let text = "ARP UAE225 8500N11600W 1457 F360 MS63 320/20=";
        let bulletin = parse_pirep_bulletin(text).expect("arp-style pirep should parse");

        assert_eq!(bulletin.reports.len(), 1);
        assert_eq!(bulletin.reports[0].time.as_deref(), Some("1457"));
        assert_eq!(bulletin.reports[0].flight_level_ft, Some(36_000));
        assert_eq!(bulletin.reports[0].temperature_c, Some(-63));
    }
}
