//! Structured METAR bulletin parsing for WMO collectives without AFOS PIL lines.

use crate::ProductParseIssue;
use serde::Serialize;

/// Type of METAR report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetarReportKind {
    /// Routine METAR observation
    Metar,
    /// Special (non-routine) observation
    Speci,
}

/// Wind block from a METAR observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MetarWind {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction_degrees: Option<u16>,
    pub speed_kt: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gust_kt: Option<u16>,
    pub is_variable: bool,
}

/// Sky condition group from a METAR observation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MetarSkyCondition {
    pub cover: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height_ft_agl: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modifier: Option<String>,
}

/// Individual METAR report from a single station.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MetarReport {
    /// Type of report (METAR or SPECI)
    pub kind: MetarReportKind,
    /// ICAO station identifier (e.g., `KBOS`)
    pub station: String,
    /// Observation time in `HHMMSSZ` format
    pub observation_time: String,
    /// True when `COR` was present in the header
    pub correction: bool,
    /// Parsed wind group
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wind: Option<MetarWind>,
    /// Parsed visibility token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
    /// Present-weather tokens such as `-RA`
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weather: Vec<String>,
    /// Parsed sky groups
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sky_conditions: Vec<MetarSkyCondition>,
    /// Air temperature in Celsius
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature_c: Option<i16>,
    /// Dewpoint in Celsius
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dewpoint_c: Option<i16>,
    /// Altimeter setting such as `Q1029` or `A3017`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub altimeter: Option<String>,
    /// Remainder beginning with `RMK`, when present
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remarks: Option<String>,
    /// Complete raw METAR text
    pub raw: String,
}

/// METAR bulletin containing multiple station reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MetarBulletin {
    /// Individual METAR reports in the bulletin
    pub reports: Vec<MetarReport>,
}

impl MetarBulletin {
    /// Returns the number of reports in the bulletin.
    pub fn report_count(&self) -> usize {
        self.reports.len()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedMetarRef {
    kind: MetarReportKind,
    station: String,
    observation_time: String,
    correction: bool,
    wind: Option<MetarWind>,
    visibility: Option<String>,
    weather: Vec<String>,
    sky_conditions: Vec<MetarSkyCondition>,
    temperature_c: Option<i16>,
    dewpoint_c: Option<i16>,
    altimeter: Option<String>,
    remarks: Option<String>,
}

impl ParsedMetarRef {
    fn into_owned(self, raw: String) -> MetarReport {
        MetarReport {
            kind: self.kind,
            station: self.station,
            observation_time: self.observation_time,
            correction: self.correction,
            wind: self.wind,
            visibility: self.visibility,
            weather: self.weather,
            sky_conditions: self.sky_conditions,
            temperature_c: self.temperature_c,
            dewpoint_c: self.dewpoint_c,
            altimeter: self.altimeter,
            remarks: self.remarks,
            raw,
        }
    }
}

/// Parses a METAR bulletin from text content.
pub(crate) fn parse_metar_bulletin(text: &str) -> Option<(MetarBulletin, Vec<ProductParseIssue>)> {
    let content = normalize_metar_segment(text);
    let mut reports = Vec::new();
    let mut issues = Vec::new();
    let mut collective_kind = None;

    for segment in content.split('=') {
        let normalized = normalize_metar_segment(segment);
        if normalized.is_empty() {
            continue;
        }
        if normalized == "METAR" {
            collective_kind = Some(MetarReportKind::Metar);
            continue;
        }
        if normalized == "SPECI" {
            collective_kind = Some(MetarReportKind::Speci);
            continue;
        }

        match parse_metar_report_ref(&normalized) {
            Some(parsed) => {
                collective_kind = Some(parsed.kind);
                reports.push(parsed.into_owned(normalized.clone()));
            }
            None => {
                if collective_kind.is_none() {
                    let tokens = normalized.split_whitespace().collect::<Vec<_>>();
                    collective_kind = find_metar_start(&tokens).map(|(kind, _, _)| kind);
                }
                if is_nil_only_station_segment(&normalized) {
                    if normalized.starts_with("METAR ") {
                        collective_kind = Some(MetarReportKind::Metar);
                    } else if normalized.starts_with("SPECI ") {
                        collective_kind = Some(MetarReportKind::Speci);
                    }
                    continue;
                }
                if let Some(parsed) = parse_surface_sa_report_ref(&normalized) {
                    collective_kind = Some(MetarReportKind::Metar);
                    reports.push(parsed.into_owned(normalized.clone()));
                    continue;
                }
                if let Some(parsed) =
                    collective_kind.and_then(|kind| parse_bare_metar_report_ref(&normalized, kind))
                {
                    reports.push(parsed.into_owned(normalized.clone()));
                    continue;
                }
                if normalized.contains("METAR") || normalized.contains("SPECI") {
                    issues.push(ProductParseIssue::new(
                        "metar_parse",
                        "invalid_metar_report",
                        "could not parse METAR/SPECI report from bulletin token",
                        Some(normalized),
                    ));
                }
            }
        }
    }

    (!reports.is_empty()).then_some((MetarBulletin { reports }, issues))
}

/// Normalizes whitespace in a segment by compacting ASCII separators in one pass.
fn normalize_metar_segment(segment: &str) -> String {
    let mut normalized = String::with_capacity(segment.len());
    let mut pending_space = false;

    for ch in segment.chars() {
        if ch.is_ascii_control() && !ch.is_ascii_whitespace() {
            continue;
        }
        if ch.is_ascii_whitespace() {
            pending_space = true;
            continue;
        }

        if pending_space && !normalized.is_empty() {
            normalized.push(' ');
        }
        pending_space = false;
        normalized.push(ch);
    }

    normalized
}

fn is_nil_only_station_segment(segment: &str) -> bool {
    let tokens = segment.split_whitespace().collect::<Vec<_>>();
    matches!(
        tokens.as_slice(),
        [_, "NIL"] | ["METAR", _, "NIL"] | ["SPECI", _, "NIL"]
    )
}

/// Parses a normalized METAR/SPECI segment into owned header fields.
fn parse_metar_report_ref(segment: &str) -> Option<ParsedMetarRef> {
    let tokens = segment.split(' ').collect::<Vec<_>>();
    let (kind, start, inline_station) = find_metar_start(&tokens)?;
    let mut index = start + 1;
    let mut correction = false;

    let station = inline_station.unwrap_or_else(|| {
        let token = tokens.get(index).copied().unwrap_or_default();
        index += 1;
        token
    });
    if station.is_empty() {
        return None;
    }
    if station == "COR" {
        correction = true;
        let next = *tokens.get(index)?;
        index += 1;
        if !is_metar_station(next) {
            return None;
        }
        return parse_metar_body(kind, next, &tokens[index..], correction, segment);
    }

    parse_metar_body(kind, station, &tokens[index..], correction, segment)
}

fn parse_bare_metar_report_ref(segment: &str, kind: MetarReportKind) -> Option<ParsedMetarRef> {
    let tokens = segment.split(' ').collect::<Vec<_>>();
    let mut index = 0;
    let mut correction = false;
    let station = *tokens.get(index)?;
    index += 1;
    if station == "COR" {
        correction = true;
        let next = *tokens.get(index)?;
        index += 1;
        if !is_metar_station(next) {
            return None;
        }
        return parse_metar_body(kind, next, &tokens[index..], correction, segment);
    }
    parse_metar_body(kind, station, &tokens[index..], correction, segment)
}

fn parse_surface_sa_report_ref(segment: &str) -> Option<ParsedMetarRef> {
    let tokens = segment.split_whitespace().collect::<Vec<_>>();
    let station = *tokens.first()?;
    let report_marker = *tokens.get(1)?;
    let hhmm = *tokens.get(2)?;
    if !is_sa_station(station) || report_marker != "SA" || hhmm.len() != 4 {
        return None;
    }

    Some(ParsedMetarRef {
        kind: MetarReportKind::Metar,
        station: station.to_string(),
        observation_time: format!("{hhmm}00Z"),
        correction: false,
        wind: None,
        visibility: None,
        weather: Vec::new(),
        sky_conditions: Vec::new(),
        temperature_c: None,
        dewpoint_c: None,
        altimeter: None,
        remarks: None,
    })
}

fn is_sa_station(token: &str) -> bool {
    (3..=4).contains(&token.len()) && token.chars().all(|ch| ch.is_ascii_uppercase())
}

fn parse_metar_body(
    kind: MetarReportKind,
    station: &str,
    body_tokens: &[&str],
    mut correction: bool,
    _raw: &str,
) -> Option<ParsedMetarRef> {
    let normalized_tokens = expand_metar_body_tokens(body_tokens);
    let mut index = 0;
    if normalized_tokens
        .get(index)
        .is_some_and(|token| token == "COR")
    {
        correction = true;
        index += 1;
    }
    let observation_time = normalized_tokens.get(index)?;
    index += 1;
    if !is_metar_station(station) || !is_observation_time(observation_time) {
        return None;
    }
    if normalized_tokens
        .get(index)
        .is_some_and(|token| token == "NIL")
    {
        return Some(ParsedMetarRef {
            kind,
            station: station.to_string(),
            observation_time: observation_time.to_string(),
            correction,
            wind: None,
            visibility: None,
            weather: Vec::new(),
            sky_conditions: Vec::new(),
            temperature_c: None,
            dewpoint_c: None,
            altimeter: None,
            remarks: None,
        });
    }

    let mut wind = None;
    let mut visibility = None;
    let mut weather = Vec::new();
    let mut sky_conditions = Vec::new();
    let mut temperature_c = None;
    let mut dewpoint_c = None;
    let mut altimeter = None;
    let mut remarks_tokens = Vec::new();
    let mut in_remarks = false;

    for token in &normalized_tokens[index..] {
        if token == "COR" {
            continue;
        }
        if in_remarks {
            remarks_tokens.push(token.to_string());
            continue;
        }
        if token == "RMK" {
            in_remarks = true;
            continue;
        }
        if wind.is_none()
            && let Some(parsed) = parse_wind(token)
        {
            wind = Some(parsed);
            continue;
        }
        if visibility.is_none() && is_visibility_token(token) {
            visibility = Some(token.to_string());
            continue;
        }
        if temperature_c.is_none()
            && dewpoint_c.is_none()
            && let Some((temperature, dewpoint)) = parse_temperature_pair(token)
        {
            temperature_c = Some(temperature);
            dewpoint_c = dewpoint;
            continue;
        }
        if altimeter.is_none() && is_altimeter_token(token) {
            altimeter = Some(token.to_string());
            continue;
        }
        if let Some(condition) = parse_sky_condition(token) {
            sky_conditions.push(condition);
            continue;
        }
        if is_weather_token(token) {
            weather.push(token.to_string());
        }
    }

    Some(ParsedMetarRef {
        kind,
        station: station.to_string(),
        observation_time: observation_time.to_string(),
        correction,
        wind,
        visibility,
        weather,
        sky_conditions,
        temperature_c,
        dewpoint_c,
        altimeter,
        remarks: (!remarks_tokens.is_empty()).then_some(remarks_tokens.join(" ")),
    })
}

fn expand_metar_body_tokens(tokens: &[&str]) -> Vec<String> {
    let mut expanded = Vec::with_capacity(tokens.len());
    for token in tokens {
        if let Some((observation_time, suffix)) = split_observation_suffix(token) {
            expanded.push(observation_time.to_string());
            if !suffix.is_empty() {
                expanded.push(suffix.to_string());
            }
        } else {
            expanded.push((*token).to_string());
        }
    }
    expanded
}

fn split_observation_suffix(token: &str) -> Option<(&str, &str)> {
    let observation = token.get(..7)?;
    if !is_observation_time(observation) || token.len() == 7 {
        return None;
    }
    Some((observation, token.get(7..).unwrap_or_default()))
}

fn find_metar_start<'a>(
    tokens: &'a [&'a str],
) -> Option<(MetarReportKind, usize, Option<&'a str>)> {
    for (index, token) in tokens.iter().copied().enumerate() {
        match token {
            "METAR" => return Some((MetarReportKind::Metar, index, None)),
            "SPECI" => return Some((MetarReportKind::Speci, index, None)),
            _ => {}
        }

        if let Some(station) = token.strip_prefix("METAR")
            && is_metar_station(station)
        {
            return Some((MetarReportKind::Metar, index, Some(station)));
        }

        if let Some(station) = token.strip_prefix("SPECI")
            && is_metar_station(station)
        {
            return Some((MetarReportKind::Speci, index, Some(station)));
        }
    }

    None
}

fn parse_wind(token: &str) -> Option<MetarWind> {
    if !token.ends_with("KT") {
        return None;
    }
    let core = token.strip_suffix("KT")?;
    let (direction, remainder, is_variable) = if let Some(rest) = core.strip_prefix("VRB") {
        (None, rest, true)
    } else {
        if core.len() < 5 {
            return None;
        }
        let (direction, rest) = core.split_at(3);
        (Some(direction.parse::<u16>().ok()?), rest, false)
    };
    let (speed, gust_kt) = if let Some((speed, gust)) = remainder.split_once('G') {
        (speed.parse::<u16>().ok()?, Some(gust.parse::<u16>().ok()?))
    } else {
        (remainder.parse::<u16>().ok()?, None)
    };
    Some(MetarWind {
        direction_degrees: direction,
        speed_kt: speed,
        gust_kt,
        is_variable,
    })
}

fn is_visibility_token(token: &str) -> bool {
    token == "CAVOK"
        || token == "9999"
        || token == "9999NDV"
        || token.ends_with("SM")
        || token.chars().all(|ch| ch.is_ascii_digit()) && token.len() == 4
}

fn is_weather_token(token: &str) -> bool {
    let token = token.trim_matches('+').trim_matches('-');
    token.len() >= 2
        && token.len() <= 8
        && token.chars().all(|ch| ch.is_ascii_uppercase())
        && !is_altimeter_token(token)
        && !matches!(
            token,
            "AUTO" | "NOSIG" | "RMK" | "COR" | "AO1" | "AO2" | "CLR" | "SKC"
        )
}

fn parse_sky_condition(token: &str) -> Option<MetarSkyCondition> {
    let cover = token.get(..3)?;
    if !matches!(
        cover,
        "SKC" | "CLR" | "FEW" | "SCT" | "BKN" | "OVC" | "VV0" | "VV/"
    ) {
        return None;
    }
    let height = token
        .get(3..6)
        .filter(|digits| digits.chars().all(|ch| ch.is_ascii_digit()))
        .and_then(|digits| digits.parse::<u32>().ok())
        .map(|height| height * 100);
    let modifier = token
        .get(6..)
        .filter(|suffix| !suffix.is_empty())
        .map(str::to_string);
    Some(MetarSkyCondition {
        cover: cover.to_string(),
        height_ft_agl: height,
        modifier,
    })
}

fn parse_temperature_pair(token: &str) -> Option<(i16, Option<i16>)> {
    let (temperature, dewpoint) = token.split_once('/')?;
    Some((
        parse_signed_temperature(temperature)?,
        parse_signed_temperature(dewpoint),
    ))
}

fn parse_signed_temperature(token: &str) -> Option<i16> {
    if token.is_empty() || token == "//" || token.contains('/') {
        return None;
    }
    let negative = token.starts_with('M') || token.starts_with('-');
    let digits = token.trim_start_matches('M').trim_start_matches('-');
    let value = digits.parse::<i16>().ok()?;
    Some(if negative { -value } else { value })
}

fn is_altimeter_token(token: &str) -> bool {
    (token.starts_with('Q') && token.len() == 5 || token.starts_with('A') && token.len() == 5)
        && token[1..].chars().all(|ch| ch.is_ascii_digit())
}

fn is_metar_station(token: &str) -> bool {
    token.len() == 4
        && token.starts_with(|ch: char| ch.is_ascii_uppercase())
        && token.chars().all(|ch| ch.is_ascii_alphanumeric())
}

fn is_observation_time(token: &str) -> bool {
    token.len() == 7 && token.ends_with('Z') && token[..6].chars().all(|ch| ch.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::{MetarReportKind, parse_metar_bulletin};

    #[test]
    fn parses_collective_with_single_metar() {
        let text = "000 \nSAGL31 BGGH 070200\nMETAR BGKK 070220Z AUTO VRB02KT 9999NDV OVC043/// M03/M08 Q0967=\n";
        let (bulletin, issues) =
            parse_metar_bulletin(text).expect("expected METAR bulletin parsing to succeed");

        assert!(issues.is_empty());
        assert_eq!(bulletin.report_count(), 1);
        assert_eq!(bulletin.reports[0].kind, MetarReportKind::Metar);
        assert_eq!(bulletin.reports[0].station, "BGKK");
        assert_eq!(bulletin.reports[0].observation_time, "070220Z");
        assert_eq!(bulletin.reports[0].visibility.as_deref(), Some("9999NDV"));
        assert_eq!(bulletin.reports[0].temperature_c, Some(-3));
        assert_eq!(bulletin.reports[0].dewpoint_c, Some(-8));
    }

    #[test]
    fn parses_multiple_reports_in_bulletin() {
        let text =
            "METAR BGKK 070220Z AUTO VRB02KT 9999= SPECI KDSM 070254Z 33007KT 10SM CLR RMK AO2=";
        let (bulletin, issues) =
            parse_metar_bulletin(text).expect("expected multiple METAR reports");

        assert!(issues.is_empty());
        assert_eq!(bulletin.report_count(), 2);
        assert_eq!(bulletin.reports[1].kind, MetarReportKind::Speci);
        assert_eq!(bulletin.reports[1].station, "KDSM");
        assert_eq!(bulletin.reports[1].remarks.as_deref(), Some("AO2"));
    }

    #[test]
    fn rejects_non_metar_body() {
        let text = "000 \nFXUS61 KBOX 022101\nAREA FORECAST DISCUSSION\n";
        assert!(parse_metar_bulletin(text).is_none());
    }

    #[test]
    fn parses_corrected_metar_report() {
        let text = "METAR COR UGKO 090030Z 24007KT 9999 SCT030 BKN061 03/01 Q1029 NOSIG=";
        let (bulletin, issues) =
            parse_metar_bulletin(text).expect("expected corrected METAR report");

        assert_eq!(bulletin.report_count(), 1);
        assert_eq!(bulletin.reports[0].station, "UGKO");
        assert!(bulletin.reports[0].correction);
        assert!(issues.is_empty());
    }

    #[test]
    fn parses_corrected_speci_report() {
        let text = "SPECI COR KDSM 070254Z 33007KT 10SM CLR M09/M14 A3017=";
        let (bulletin, issues) =
            parse_metar_bulletin(text).expect("expected corrected SPECI report");

        assert_eq!(bulletin.report_count(), 1);
        assert_eq!(bulletin.reports[0].station, "KDSM");
        assert_eq!(bulletin.reports[0].altimeter.as_deref(), Some("A3017"));
        assert!(issues.is_empty());
    }

    #[test]
    fn parses_station_led_corrected_metar_report() {
        let text = "METAR TBPB COR 172000Z 11021KT 9999 SCT020 28/20 Q1013 NOSIG=";
        let (bulletin, issues) =
            parse_metar_bulletin(text).expect("expected station-led corrected METAR report");

        assert!(issues.is_empty());
        assert_eq!(bulletin.report_count(), 1);
        assert_eq!(bulletin.reports[0].station, "TBPB");
        assert_eq!(bulletin.reports[0].observation_time, "172000Z");
        assert!(bulletin.reports[0].correction);
    }

    #[test]
    fn parses_fused_observation_suffix_token() {
        let text = "METAR SBOI 171800ZAUTO 04011KT 360V080 9999 FEW021 SCT026 28/23 Q1009=";
        let (bulletin, issues) =
            parse_metar_bulletin(text).expect("expected fused observation suffix METAR report");

        assert!(issues.is_empty());
        assert_eq!(bulletin.report_count(), 1);
        assert_eq!(bulletin.reports[0].station, "SBOI");
        assert_eq!(bulletin.reports[0].observation_time, "171800Z");
        assert_eq!(
            bulletin.reports[0].wind.as_ref().map(|wind| wind.speed_kt),
            Some(11)
        );
    }

    #[test]
    fn parses_nil_metar_report() {
        let text = "METAR TGPY 281600Z NIL=";
        let (bulletin, issues) = parse_metar_bulletin(text).expect("expected NIL METAR report");

        assert!(issues.is_empty());
        assert_eq!(bulletin.report_count(), 1);
        assert_eq!(bulletin.reports[0].station, "TGPY");
        assert_eq!(bulletin.reports[0].observation_time, "281600Z");
        assert_eq!(bulletin.reports[0].raw, "METAR TGPY 281600Z NIL");
        assert!(bulletin.reports[0].wind.is_none());
    }

    #[test]
    fn invalid_metar_token_emits_issue() {
        let text = "METAR BAD 070254Z=METAR KDSM 070254Z AUTO CLR=";
        let (_, issues) = parse_metar_bulletin(text)
            .expect("expected issue-bearing bulletin with one valid report");

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "invalid_metar_report");
    }

    #[test]
    fn skips_nil_only_station_tokens_inside_collective() {
        let text = "METAR MBJT NIL=MBPV 080000Z 11002KT 9999 FEW012 25/22 Q1012=MBSC NIL=";
        let (bulletin, issues) =
            parse_metar_bulletin(text).expect("expected mixed collective bulletin");

        assert!(issues.is_empty());
        assert_eq!(bulletin.report_count(), 1);
        assert_eq!(bulletin.reports[0].station, "MBPV");
    }

    #[test]
    fn raw_report_uses_compacted_whitespace() {
        let text = "METAR   BGKK   070220Z   AUTO   VRB02KT=";
        let (bulletin, _) = parse_metar_bulletin(text).expect("expected METAR bulletin");

        assert_eq!(bulletin.reports[0].raw, "METAR BGKK 070220Z AUTO VRB02KT");
    }

    #[test]
    fn parses_compact_metar_prefix() {
        let text = "METARSBUF 112000Z AUTO 13006KT CAVOK 32/19 Q1009=";
        let (bulletin, issues) =
            parse_metar_bulletin(text).expect("expected compact METAR bulletin");

        assert!(issues.is_empty());
        assert_eq!(bulletin.report_count(), 1);
        assert_eq!(bulletin.reports[0].station, "SBUF");
        assert_eq!(bulletin.reports[0].visibility.as_deref(), Some("CAVOK"));
    }
}
