//! NWS VTEC (Valid Time Event Code) parsing module.
//!
//! VTEC codes are simple slash-delimited records, so this parser scans for
//! candidate blocks and parses their dot-delimited fields directly.

use chrono::{DateTime, Datelike, NaiveDateTime, Utc};

use crate::ProductParseIssue;
use crate::body::support::scan_slash_delimited_candidates;

/// A parsed VTEC code.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct VtecCode {
    pub status: char,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_description: Option<&'static str>,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_description: Option<&'static str>,
    pub office: String,
    pub phenomena: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phenomena_description: Option<&'static str>,
    pub significance: char,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub significance_description: Option<&'static str>,
    pub etn: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub begin: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<DateTime<Utc>>,
}

fn vtec_phenomena_description(code: &str) -> Option<&'static str> {
    match code {
        "AF" => Some("Ashfall"),
        "AS" => Some("Air Stagnation"),
        "AV" => Some("Avalanche"),
        "BH" => Some("Beach Hazard"),
        "BS" => Some("Blowing Snow"),
        "BW" => Some("Brisk Wind"),
        "BZ" => Some("Blizzard"),
        "CF" => Some("Coastal Flood"),
        "CW" => Some("Cold Weather"),
        "DF" => Some("Debris Flow"),
        "DS" => Some("Dust Storm"),
        "DU" => Some("Blowing Dust"),
        "EC" => Some("Extreme Cold"),
        "EH" => Some("Excessive Heat"),
        "EW" => Some("Extreme Wind"),
        "FA" => Some("Areal Flood"),
        "FF" => Some("Flash Flood"),
        "FG" => Some("Dense Fog"),
        "FL" => Some("Flood"),
        "FR" => Some("Frost"),
        "FW" => Some("Fire Weather"),
        "FZ" => Some("Freeze"),
        "GL" => Some("Gale"),
        "HF" => Some("Hurricane Force Wind"),
        "HI" => Some("Inland Hurricane Wind"),
        "HS" => Some("Heavy Snow"),
        "HT" => Some("Heat"),
        "HU" => Some("Hurricane"),
        "HW" => Some("High Wind"),
        "HY" => Some("Hydrologic"),
        "HZ" => Some("Hard Freeze"),
        "IP" => Some("Sleet"),
        "IS" => Some("Ice Storm"),
        "LB" => Some("Lake Effect Snow and Blowing Snow"),
        "LE" => Some("Lake Effect Snow"),
        "LO" => Some("Low Water"),
        "LS" => Some("Lakeshore Flood"),
        "LW" => Some("Lake Wind"),
        "MA" => Some("Marine"),
        "MF" => Some("Marine Dense Fog"),
        "MH" => Some("Marine Ashfall"),
        "MS" => Some("Marine Dense Smoke"),
        "RB" => Some("Small Craft for Rough Bar"),
        "RP" => Some("Rip Currents"),
        "SB" => Some("Snow And Blowing Snow"),
        "SC" => Some("Small Craft"),
        "SE" => Some("Hazardous Seas"),
        "SI" => Some("Small Craft for Winds"),
        "SM" => Some("Smoke"),
        "SN" => Some("Snow"),
        "SQ" => Some("Snow Squall"),
        "SR" => Some("Storm"),
        "SS" => Some("Storm Surge"),
        "SU" => Some("High Surf"),
        "SV" => Some("Severe Thunderstorm"),
        "SW" => Some("Small Craft for Hazardous Seas"),
        "TI" => Some("Inland Tropical Storm Wind"),
        "TO" => Some("Tornado"),
        "TR" => Some("Tropical Storm"),
        "TS" => Some("Tsunami"),
        "TY" => Some("Typhoon"),
        "UP" => Some("Freezing Spray"),
        "WC" => Some("Wind Chill"),
        "WI" => Some("Wind"),
        "WS" => Some("Winter Storm"),
        "WW" => Some("Winter Weather"),
        "XH" => Some("Extreme Heat"),
        "ZF" => Some("Freezing Fog"),
        "ZR" => Some("Freezing Rain"),
        _ => None,
    }
}

fn vtec_action_description(code: &str) -> Option<&'static str> {
    match code {
        "NEW" => Some("New"),
        "CON" => Some("Continued"),
        "EXT" => Some("Changed in Time"),
        "EXA" => Some("Changed in Area"),
        "EXB" => Some("Changed in Time and Area"),
        "CAN" => Some("Cancelled"),
        "UPG" => Some("Upgraded"),
        "EXP" => Some("Expired"),
        "COR" => Some("Corrected"),
        "ROU" => Some("Routine"),
        _ => None,
    }
}

fn vtec_status_description(code: char) -> Option<&'static str> {
    match code {
        'O' => Some("Operational"),
        'T' => Some("Test"),
        'E' => Some("Experimental"),
        'X' => Some("Experimental VTEC"),
        _ => None,
    }
}

fn vtec_significance_description(code: char) -> Option<&'static str> {
    match code {
        'W' => Some("Warning"),
        'A' => Some("Watch"),
        'Y' => Some("Advisory"),
        'S' => Some("Statement"),
        'O' => Some("Outlook"),
        'N' => Some("Synopsis"),
        'F' => Some("Forecast"),
        _ => None,
    }
}

pub fn parse_vtec_codes(text: &str) -> Vec<VtecCode> {
    parse_vtec_codes_with_issues(text).0
}

pub fn parse_vtec_codes_with_issues(text: &str) -> (Vec<VtecCode>, Vec<ProductParseIssue>) {
    let mut codes = Vec::new();
    let mut issues = Vec::new();

    for candidate in find_vtec_candidates(text) {
        match parse_vtec_candidate(candidate) {
            Ok(code) => codes.push(code),
            Err(issue) => issues.push(issue),
        }
    }

    (codes, issues)
}

fn find_vtec_candidates(text: &str) -> Vec<&str> {
    scan_slash_delimited_candidates(text, |candidate| {
        candidate
            .as_bytes()
            .get(1)
            .copied()
            .map(|byte| (byte as char).is_ascii_uppercase())
            .unwrap_or(false)
            && candidate.as_bytes().get(2) == Some(&b'.')
    })
}

fn parse_vtec_candidate(raw: &str) -> Result<VtecCode, ProductParseIssue> {
    let inner = raw
        .strip_prefix('/')
        .and_then(|value| value.strip_suffix('/'))
        .ok_or_else(|| invalid_format_issue(raw))?;
    let fields: Vec<&str> = inner.split('.').collect();
    if fields.len() != 7 {
        return Err(invalid_format_issue(raw));
    }

    let status = fields[0]
        .chars()
        .next()
        .filter(|_| fields[0].len() == 1)
        .ok_or_else(|| invalid_format_issue(raw))?;
    let action = fields[1];
    let office = fields[2];
    let phenomena = fields[3];
    let significance = fields[4]
        .chars()
        .next()
        .filter(|_| fields[4].len() == 1)
        .ok_or_else(|| invalid_format_issue(raw))?;
    let etn = fields[5]
        .parse::<u32>()
        .map_err(|_| invalid_etn_issue(raw))?;
    let (begin_raw, end_raw) = fields[6]
        .split_once('-')
        .ok_or_else(|| invalid_format_issue(raw))?;
    let begin = parse_vtec_time_or_unspecified(begin_raw, "invalid_vtec_begin_time", raw)?;
    let end = parse_vtec_time_or_unspecified(end_raw, "invalid_vtec_end_time", raw)?;

    Ok(VtecCode {
        status,
        status_description: vtec_status_description(status),
        action: action.to_string(),
        action_description: vtec_action_description(action),
        office: office.to_string(),
        phenomena: phenomena.to_string(),
        phenomena_description: vtec_phenomena_description(phenomena),
        significance,
        significance_description: vtec_significance_description(significance),
        etn,
        begin,
        end,
    })
}

fn parse_vtec_time_or_unspecified(
    token: &str,
    code: &'static str,
    raw: &str,
) -> Result<Option<DateTime<Utc>>, ProductParseIssue> {
    if token == "000000T0000Z" {
        return Ok(None);
    }

    let parsed = parse_vtec_time(token).ok_or_else(|| {
        ProductParseIssue::new(
            "vtec_parse",
            code,
            format!(
                "could not parse VTEC {} from code: `{raw}`",
                if code == "invalid_vtec_begin_time" {
                    "begin time"
                } else {
                    "end time"
                }
            ),
            Some(raw.to_string()),
        )
    })?;

    if parsed.year() < 1971 {
        return Ok(None);
    }

    Ok(Some(parsed))
}

fn parse_vtec_time(time_str: &str) -> Option<DateTime<Utc>> {
    if time_str.len() != 12 {
        return None;
    }
    let naive = NaiveDateTime::parse_from_str(time_str, "%y%m%dT%H%MZ").ok()?;
    Some(naive.and_utc())
}

fn invalid_format_issue(raw: &str) -> ProductParseIssue {
    ProductParseIssue::new(
        "vtec_parse",
        "invalid_vtec_format",
        format!("could not parse VTEC code: `{raw}`"),
        Some(raw.to_string()),
    )
}

fn invalid_etn_issue(raw: &str) -> ProductParseIssue {
    ProductParseIssue::new(
        "vtec_parse",
        "invalid_vtec_etn",
        format!("could not parse VTEC ETN from code: `{raw}`"),
        Some(raw.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::{parse_vtec_codes, parse_vtec_codes_with_issues};

    #[test]
    fn parse_single_vtec() {
        let text = "/O.NEW.KDMX.TO.W.0123.250301T1200Z-250301T1300Z/";
        let codes = parse_vtec_codes(text);

        assert_eq!(codes.len(), 1);
        assert_eq!(codes[0].status, 'O');
        assert_eq!(codes[0].status_description, Some("Operational"));
        assert_eq!(codes[0].action, "NEW");
        assert_eq!(codes[0].office, "KDMX");
        assert_eq!(codes[0].phenomena, "TO");
        assert_eq!(codes[0].phenomena_description, Some("Tornado"));
        assert_eq!(codes[0].significance, 'W');
        assert_eq!(codes[0].etn, 123);
    }

    #[test]
    fn parse_multiple_vtec() {
        let text = concat!(
            "/O.NEW.KDMX.TO.W.0123.250301T1200Z-250301T1300Z/",
            " ... ",
            "/O.CON.KTOP.SV.W.0045.250301T1300Z-250301T1400Z/"
        );
        let codes = parse_vtec_codes(text);

        assert_eq!(codes.len(), 2);
        assert_eq!(codes[0].office, "KDMX");
        assert_eq!(codes[1].office, "KTOP");
    }

    #[test]
    fn parse_vtec_with_unspecified_times() {
        let text = "/O.CON.KGID.SV.W.0001.000000T0000Z-000000T0000Z/";
        let codes = parse_vtec_codes(text);

        assert_eq!(codes.len(), 1);
        assert_eq!(codes[0].begin, None);
        assert_eq!(codes[0].end, None);
    }

    #[test]
    fn parse_vtec_unknown_phenomena_has_no_description() {
        let text = "/O.NEW.KDMX.XX.W.0123.250301T1200Z-250301T1300Z/";
        let codes = parse_vtec_codes(text);

        assert_eq!(codes.len(), 1);
        assert_eq!(codes[0].phenomena_description, None);
    }

    #[test]
    fn parse_vtec_supports_experimental_vtec_status() {
        let text = "/X.NEW.KDMX.TO.W.0123.250301T1200Z-250301T1300Z/";
        let codes = parse_vtec_codes(text);

        assert_eq!(codes.len(), 1);
        assert_eq!(codes[0].status_description, Some("Experimental VTEC"));
    }

    #[test]
    fn candidate_scan_ignores_malformed_slash_blocks() {
        let text = "/NOTVTEC/ /O.BAD/";
        let codes = parse_vtec_codes(text);
        assert!(codes.is_empty());
    }

    #[test]
    fn multiple_vtec_codes_in_one_line_parse() {
        let text = "/O.NEW.KDMX.TO.W.0001.250301T1200Z-250301T1300Z/ /O.NEW.KDMX.SV.W.0002.250301T1200Z-250301T1300Z/";
        let codes = parse_vtec_codes(text);
        assert_eq!(codes.len(), 2);
    }

    #[test]
    fn invalid_etn_reports_issue() {
        let text = "/O.NEW.KDMX.TO.W.ABCD.250301T1200Z-250301T1300Z/";
        let (_codes, issues) = parse_vtec_codes_with_issues(text);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "invalid_vtec_etn");
    }

    #[test]
    fn malformed_begin_end_split_reports_invalid_format() {
        let text = "/O.NEW.KDMX.TO.W.0001.250301T1200Z250301T1300Z/";
        let (_codes, issues) = parse_vtec_codes_with_issues(text);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "invalid_vtec_format");
    }

    #[test]
    fn unknown_status_action_and_phenomena_still_parse() {
        let text = "/Q.ZZZ.KDMX.XX.Q.0001.250301T1200Z-250301T1300Z/";
        let codes = parse_vtec_codes(text);

        assert_eq!(codes.len(), 1);
        assert_eq!(codes[0].status_description, None);
        assert_eq!(codes[0].action_description, None);
        assert_eq!(codes[0].phenomena_description, None);
        assert_eq!(codes[0].significance_description, None);
    }
}
