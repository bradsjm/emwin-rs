//! NWS HVTEC (Hydrologic VTEC) parsing module.
//!
//! HVTEC blocks are slash- and dot-delimited, so they are parsed directly
//! without regex capture machinery.

use chrono::{DateTime, NaiveDateTime, Utc};

use crate::ProductParseIssue;
use crate::body::support::scan_slash_delimited_candidates;
use crate::data::{NwslidEntry, nwslid_entry};

/// A parsed HVTEC code.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct HvtecCode {
    pub nwslid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<NwslidEntry>,
    pub severity: HvtecSeverity,
    pub cause: HvtecCause,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub begin: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crest: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<DateTime<Utc>>,
    pub record: HvtecRecord,
}

/// Severity levels for HVTEC flood events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum HvtecSeverity {
    None,
    Minor,
    Moderate,
    Major,
    Unknown,
}

impl HvtecSeverity {
    fn from_str(s: &str) -> Self {
        match s {
            "N" | "0" => HvtecSeverity::None,
            "1" => HvtecSeverity::Minor,
            "2" => HvtecSeverity::Moderate,
            "3" => HvtecSeverity::Major,
            "U" => HvtecSeverity::Unknown,
            _ => HvtecSeverity::Unknown,
        }
    }
}

/// Immediate cause codes for HVTEC events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum HvtecCause {
    ExcessiveRainfall,
    Snowmelt,
    RainAndSnowmelt,
    DamFailure,
    GlacierOutburst,
    IceJam,
    RainSnowmeltIceJam,
    UpstreamFloodingStormSurge,
    UpstreamFloodingTidalEffects,
    ElevatedUpstreamFlowTidalEffects,
    WindTidalEffects,
    UpstreamDamRelease,
    MultipleCauses,
    OtherEffects,
    Unknown,
    Other,
}

impl HvtecCause {
    fn from_str(s: &str) -> Self {
        match s {
            "ER" => HvtecCause::ExcessiveRainfall,
            "SM" => HvtecCause::Snowmelt,
            "RS" => HvtecCause::RainAndSnowmelt,
            "DM" => HvtecCause::DamFailure,
            "GO" => HvtecCause::GlacierOutburst,
            "IJ" => HvtecCause::IceJam,
            "IC" => HvtecCause::RainSnowmeltIceJam,
            "FS" => HvtecCause::UpstreamFloodingStormSurge,
            "FT" => HvtecCause::UpstreamFloodingTidalEffects,
            "ET" => HvtecCause::ElevatedUpstreamFlowTidalEffects,
            "WT" => HvtecCause::WindTidalEffects,
            "DR" => HvtecCause::UpstreamDamRelease,
            "MC" => HvtecCause::MultipleCauses,
            "OT" => HvtecCause::OtherEffects,
            "UU" => HvtecCause::Unknown,
            _ => HvtecCause::Other,
        }
    }
}

/// Record status indicators for HVTEC events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum HvtecRecord {
    NoRecord,
    NearRecord,
    NotApplicable,
    Unavailable,
    Unknown,
}

impl HvtecRecord {
    fn from_str(s: &str) -> Self {
        match s {
            "NO" => HvtecRecord::NoRecord,
            "NR" => HvtecRecord::NearRecord,
            "OO" => HvtecRecord::NotApplicable,
            "UU" => HvtecRecord::Unavailable,
            _ => HvtecRecord::Unknown,
        }
    }
}

pub fn parse_hvtec_codes(text: &str) -> Vec<HvtecCode> {
    parse_hvtec_codes_with_issues(text).0
}

pub fn parse_hvtec_codes_with_issues(text: &str) -> (Vec<HvtecCode>, Vec<ProductParseIssue>) {
    let mut codes = Vec::new();
    let mut issues = Vec::new();

    for candidate in find_hvtec_candidates(text) {
        match parse_hvtec_candidate(candidate) {
            Ok(code) => codes.push(code),
            Err(issue) => issues.push(issue),
        }
    }

    (codes, issues)
}

fn find_hvtec_candidates(text: &str) -> Vec<&str> {
    scan_slash_delimited_candidates(text, is_structurally_hvtec_candidate)
}

fn is_structurally_hvtec_candidate(candidate: &str) -> bool {
    let Some(inner) = candidate
        .strip_prefix('/')
        .and_then(|value| value.strip_suffix('/'))
    else {
        return false;
    };

    let fields = inner.split('.').collect::<Vec<_>>();
    if fields.len() < 3 {
        return false;
    }

    is_valid_nwslid(fields[0])
        && matches!(fields[1], "N" | "0" | "1" | "2" | "3" | "U")
        && fields[2].len() == 2
        && fields[2]
            .chars()
            .all(|character| character.is_ascii_uppercase())
}

fn is_valid_nwslid(field: &str) -> bool {
    field.len() == 5
        && field
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

fn parse_hvtec_candidate(raw: &str) -> Result<HvtecCode, ProductParseIssue> {
    let inner = raw
        .strip_prefix('/')
        .and_then(|value| value.strip_suffix('/'))
        .ok_or_else(|| invalid_format_issue(raw))?;
    let fields: Vec<&str> = inner.split('.').collect();
    if fields.len() != 7 {
        return Err(invalid_format_issue(raw));
    }

    let nwslid = fields[0].to_string();
    let location = nwslid_entry(&nwslid).copied();
    let severity = HvtecSeverity::from_str(fields[1]);
    let cause = HvtecCause::from_str(fields[2]);
    let begin = parse_hvtec_optional_time(fields[3])
        .ok_or_else(|| invalid_time_issue("invalid_hvtec_begin_time", raw))?;
    let crest = parse_hvtec_optional_time(fields[4])
        .ok_or_else(|| invalid_time_issue("invalid_hvtec_crest_time", raw))?;
    let end = parse_hvtec_optional_time(fields[5])
        .ok_or_else(|| invalid_time_issue("invalid_hvtec_end_time", raw))?;
    let record = HvtecRecord::from_str(fields[6]);

    Ok(HvtecCode {
        nwslid,
        location,
        severity,
        cause,
        begin,
        crest,
        end,
        record,
    })
}

fn parse_hvtec_optional_time(token: &str) -> Option<Option<DateTime<Utc>>> {
    if token == "000000T0000Z" {
        return Some(None);
    }
    parse_hvtec_time(token).map(Some)
}

fn parse_hvtec_time(time_str: &str) -> Option<DateTime<Utc>> {
    if time_str.len() != 12 {
        return None;
    }
    let naive = NaiveDateTime::parse_from_str(time_str, "%y%m%dT%H%MZ").ok()?;
    Some(naive.and_utc())
}

fn invalid_format_issue(raw: &str) -> ProductParseIssue {
    ProductParseIssue::new(
        "hvtec_parse",
        "invalid_hvtec_format",
        format!("could not parse HVTEC code: `{raw}`"),
        Some(raw.to_string()),
    )
}

fn invalid_time_issue(code: &'static str, raw: &str) -> ProductParseIssue {
    ProductParseIssue::new(
        "hvtec_parse",
        code,
        format!(
            "could not parse HVTEC {} from code: `{raw}`",
            match code {
                "invalid_hvtec_begin_time" => "begin time",
                "invalid_hvtec_crest_time" => "crest time",
                _ => "end time",
            }
        ),
        Some(raw.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        HvtecCause, HvtecRecord, HvtecSeverity, parse_hvtec_codes, parse_hvtec_codes_with_issues,
    };

    #[test]
    fn parse_single_hvtec() {
        let text = "/MSRM1.3.ER.250301T1200Z.250301T1800Z.250302T0000Z.NO/";
        let codes = parse_hvtec_codes(text);

        assert_eq!(codes.len(), 1);
        assert_eq!(codes[0].nwslid, "MSRM1");
        assert!(codes[0].location.is_none());
        assert_eq!(codes[0].severity, HvtecSeverity::Major);
        assert_eq!(codes[0].cause, HvtecCause::ExcessiveRainfall);
        assert_eq!(codes[0].record, HvtecRecord::NoRecord);
    }

    #[test]
    fn parse_hvtec_enriches_known_nwslid() {
        let text = "/CHFA2.3.ER.250301T1200Z.250301T1800Z.250302T0000Z.NO/";
        let codes = parse_hvtec_codes(text);

        assert_eq!(codes.len(), 1);
        let location = codes[0].location.expect("expected known location");
        assert_eq!(location.nwslid, "CHFA2");
        assert_eq!(location.place_name, "Fairbanks");
    }

    #[test]
    fn parse_hvtec_supports_none_severity_and_zero_times() {
        let text = "/00000.0.ER.000000T0000Z.000000T0000Z.000000T0000Z.OO/";
        let codes = parse_hvtec_codes(text);

        assert_eq!(codes.len(), 1);
        assert_eq!(codes[0].severity, HvtecSeverity::None);
        assert_eq!(codes[0].begin, None);
        assert_eq!(codes[0].crest, None);
        assert_eq!(codes[0].end, None);
    }

    #[test]
    fn parse_hvtec_invalid_reports_issue() {
        let text = "/MSRM1.3.ER.250301T1200Z.invalid.250302T0000Z.NO/";
        let (_codes, issues) = parse_hvtec_codes_with_issues(text);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "invalid_hvtec_crest_time");
    }

    #[test]
    fn candidate_scan_ignores_malformed_slash_blocks() {
        let text = "/BROKEN/ /TOOSHORT.ER.XX/";
        let codes = parse_hvtec_codes(text);
        assert!(codes.is_empty());
    }

    #[test]
    fn multiple_hvtec_codes_in_one_body() {
        let text = concat!(
            "/MSRM1.3.ER.250301T1200Z.250301T1800Z.250302T0000Z.NO/",
            " some text ",
            "/CHFA2.2.SM.250301T1000Z.250301T1500Z.250302T0800Z.NR/"
        );
        let codes = parse_hvtec_codes(text);

        assert_eq!(codes.len(), 2);
    }

    #[test]
    fn malformed_field_count_reports_invalid_format() {
        let text = "/MSRM1.3.ER.250301T1200Z.250301T1800Z.NO/";
        let (_codes, issues) = parse_hvtec_codes_with_issues(text);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "invalid_hvtec_format");
    }

    #[test]
    fn invalid_nwslid_still_parses_without_enrichment() {
        let text = "/ABCDE.3.ER.250301T1200Z.250301T1800Z.250302T0000Z.NO/";
        let codes = parse_hvtec_codes(text);

        assert_eq!(codes.len(), 1);
        assert_eq!(codes[0].location, None);
    }

    #[test]
    fn unknown_record_maps_to_unknown() {
        let text = "/MSRM1.3.ER.250301T1200Z.250301T1800Z.250302T0000Z.XX/";
        let codes = parse_hvtec_codes(text);

        assert_eq!(codes.len(), 1);
        assert_eq!(codes[0].record, HvtecRecord::Unknown);
    }

    #[test]
    fn url_like_slash_block_is_ignored() {
        let text = "https://water.noaa.gov/wfo/SHV";
        let (codes, issues) = parse_hvtec_codes_with_issues(text);

        assert!(codes.is_empty());
        assert!(issues.is_empty());
    }
}
