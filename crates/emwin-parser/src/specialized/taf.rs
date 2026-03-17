//! Structured TAF bulletin parsing for WMO bulletins without AFOS PIL lines.

use serde::Serialize;
use winnow::Parser;
use winnow::combinator::alt;
use winnow::error::ContextError;

/// Wind block from a TAF forecast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TafWind {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction_degrees: Option<u16>,
    pub speed_kt: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gust_kt: Option<u16>,
    pub is_variable: bool,
}

/// Sky condition group from a TAF forecast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TafSkyCondition {
    pub cover: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height_ft_agl: Option<u32>,
}

/// Low-level wind-shear group from a TAF forecast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TafWindShear {
    pub height_ft_agl: u32,
    pub direction_degrees: u16,
    pub speed_kt: u16,
}

/// Parsed conditions attached to the initial TAF or a change group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct TafConditions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wind: Option<TafWind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weather: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sky_conditions: Vec<TafSkyCondition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wind_shear: Option<TafWindShear>,
}

/// TAF change-group type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TafForecastGroupKind {
    Fm,
    Tempo,
    Prob,
    Becmg,
    Inter,
}

/// Forecast or change group contained within a TAF bulletin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TafForecastGroup {
    pub change_kind: TafForecastGroupKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probability_percent: Option<u8>,
    pub conditions: TafConditions,
    pub raw: String,
}

/// TAF bulletin containing a terminal aerodrome forecast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TafBulletin {
    /// ICAO station identifier (e.g., `KBOS`)
    pub station: String,
    /// Issue time in `HHMMSSZ` format
    pub issue_time: String,
    /// Validity period start (`DDHH`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,
    /// Validity period end (`DDHH`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<String>,
    /// True if this is an amended forecast (`TAF AMD`)
    pub amendment: bool,
    /// True if this is a corrected forecast (`TAF COR`)
    pub correction: bool,
    /// Initial forecast conditions before any explicit change group
    pub initial_conditions: TafConditions,
    /// Ordered change groups
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<TafForecastGroup>,
    /// Complete raw TAF text
    pub raw: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Preamble {
    amendment: bool,
    correction: bool,
}

impl Preamble {
    fn normalized_prefix(self) -> &'static str {
        match (self.amendment, self.correction) {
            (true, false) => "TAF AMD",
            (false, true) => "TAF COR",
            (false, false) => "TAF",
            (true, true) => unreachable!("TAF preamble cannot be both amended and corrected"),
        }
    }
}

/// Parses a TAF bulletin from text content.
pub(crate) fn parse_taf_bulletin(text: &str) -> Option<TafBulletin> {
    let compact = compact_ascii_whitespace(text);
    let compact = compact.trim().trim_end_matches('=');
    let compact = strip_leading_marker_line(compact);
    let mut input = compact;
    let preamble = parse_taf_prefix(&mut input).or_else(|| {
        looks_like_station_led_taf_report(input).then_some(Preamble {
            amendment: false,
            correction: false,
        })
    })?;
    let report_body = input;
    let station = next_token(&mut input)?;
    let issue_time = next_token(&mut input)?;
    if !is_station_token(station) || !is_issue_time_token(issue_time) {
        return None;
    }

    let (valid_from, valid_to) = next_token(&mut input)
        .and_then(parse_validity_range)
        .map(|(from, to)| (Some(from.to_string()), Some(to.to_string())))
        .unwrap_or((None, None));
    let remainder = input.trim();
    let (initial_conditions, groups) = parse_forecast_groups(remainder);

    Some(TafBulletin {
        station: station.to_string(),
        issue_time: issue_time.to_string(),
        valid_from,
        valid_to,
        amendment: preamble.amendment,
        correction: preamble.correction,
        initial_conditions,
        groups,
        raw: if report_body.is_empty() {
            preamble.normalized_prefix().to_string()
        } else {
            format!("{} {}", preamble.normalized_prefix(), report_body)
        },
    })
}

fn strip_leading_marker_line(text: &str) -> &str {
    let Some((first, rest)) = text.split_once(' ') else {
        return text;
    };
    if first.len() > 3
        && first.starts_with("TAF")
        && first
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
        && rest.starts_with("TAF")
    {
        rest
    } else {
        text
    }
}

fn looks_like_station_led_taf_report(text: &str) -> bool {
    let mut parts = text.split_whitespace();
    let Some(station) = parts.next() else {
        return false;
    };
    let Some(issue_time) = parts.next() else {
        return false;
    };
    is_station_token(station) && is_issue_time_token(issue_time)
}

/// Compacts ASCII whitespace in one pass.
fn compact_ascii_whitespace(text: &str) -> String {
    let mut compacted = String::with_capacity(text.len());
    let mut pending_space = false;

    for ch in text.chars() {
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

/// Parses the TAF preamble and absorbs duplicated marker patterns.
fn parse_taf_prefix(input: &mut &str) -> Option<Preamble> {
    let preamble = alt::<_, Preamble, ContextError, _>((
        "TAF TAF AMD".value(Preamble {
            amendment: true,
            correction: false,
        }),
        "TAF TAF COR".value(Preamble {
            amendment: false,
            correction: true,
        }),
        "TAF AMD TAF AMD".value(Preamble {
            amendment: true,
            correction: false,
        }),
        "TAF COR TAF COR".value(Preamble {
            amendment: false,
            correction: true,
        }),
        "TAF AMD TAF".value(Preamble {
            amendment: true,
            correction: false,
        }),
        "TAF COR TAF".value(Preamble {
            amendment: false,
            correction: true,
        }),
        "TAF TAF".value(Preamble {
            amendment: false,
            correction: false,
        }),
        "TAF AMD".value(Preamble {
            amendment: true,
            correction: false,
        }),
        "TAF COR".value(Preamble {
            amendment: false,
            correction: true,
        }),
        "TAF".value(Preamble {
            amendment: false,
            correction: false,
        }),
    ))
    .parse_next(input)
    .ok()?;

    if input.starts_with(' ') {
        *input = &input[1..];
    }

    Some(preamble)
}

fn parse_forecast_groups(remainder: &str) -> (TafConditions, Vec<TafForecastGroup>) {
    if remainder.is_empty() {
        return (TafConditions::default(), Vec::new());
    }

    let tokens = remainder.split_whitespace().collect::<Vec<_>>();
    let mut segments = Vec::<Vec<&str>>::new();
    let mut current = Vec::new();

    for token in tokens {
        if is_group_marker(token) && !current.is_empty() {
            segments.push(std::mem::take(&mut current));
        }
        current.push(token);
    }
    if !current.is_empty() {
        segments.push(current);
    }

    let mut merged_segments = Vec::<Vec<&str>>::new();
    for segment in segments {
        if segment.len() == 1
            && segment[0].starts_with("PROB")
            && merged_segments
                .last()
                .and_then(|previous| previous.first().copied())
                .is_some_and(|token| matches!(token, "TEMPO" | "INTER" | "BECMG"))
        {
            merged_segments
                .last_mut()
                .expect("previous segment exists")
                .push(segment[0]);
            continue;
        }
        merged_segments.push(segment);
    }

    let mut index = 0;
    while index + 1 < merged_segments.len() {
        if merged_segments[index].len() == 1
            && merged_segments[index][0].starts_with("PROB")
            && merged_segments[index + 1]
                .first()
                .is_some_and(|token| matches!(*token, "TEMPO" | "INTER"))
        {
            let mut combined = vec![merged_segments[index][0]];
            combined.extend(merged_segments[index + 1].iter().copied());
            merged_segments[index] = combined;
            merged_segments.remove(index + 1);
            continue;
        }
        index += 1;
    }

    let initial = merged_segments
        .first()
        .map(|tokens| parse_conditions(tokens))
        .unwrap_or_default();
    let groups = merged_segments
        .into_iter()
        .skip(1)
        .filter_map(|segment| parse_group(&segment))
        .collect();

    (initial, groups)
}

fn parse_group(tokens: &[&str]) -> Option<TafForecastGroup> {
    let first = *tokens.first()?;
    let mut index = 1;
    let (change_kind, valid_from, valid_to, probability_percent, body_end) =
        if let Some(from) = first.strip_prefix("FM") {
            if from.len() != 6 || !from.chars().all(|ch| ch.is_ascii_digit()) {
                return None;
            }
            (
                TafForecastGroupKind::Fm,
                Some(from[..4].to_string()),
                None,
                None,
                tokens.len(),
            )
        } else if matches!(first, "TEMPO" | "BECMG" | "INTER") {
            let validity = *tokens.get(index)?;
            let (from, to) = parse_validity_range(validity)?;
            index += 1;
            let mut body_end = tokens.len();
            let probability_percent = tokens
                .last()
                .and_then(|token| token.strip_prefix("PROB"))
                .and_then(|token| token.parse::<u8>().ok())
                .filter(|_| body_end > index)
                .inspect(|_| body_end -= 1);
            (
                match first {
                    "TEMPO" => TafForecastGroupKind::Tempo,
                    "BECMG" => TafForecastGroupKind::Becmg,
                    "INTER" => TafForecastGroupKind::Inter,
                    _ => unreachable!("matched TAF group marker"),
                },
                Some(from.to_string()),
                Some(to.to_string()),
                probability_percent,
                body_end,
            )
        } else if let Some(probability) = first.strip_prefix("PROB") {
            let probability_percent = probability.parse::<u8>().ok()?;
            let next = *tokens.get(index)?;
            let (change_kind, valid_from, valid_to) = if matches!(next, "TEMPO" | "INTER") {
                index += 1;
                let validity = *tokens.get(index)?;
                let (from, to) = parse_validity_range(validity)?;
                index += 1;
                (
                    if next == "TEMPO" {
                        TafForecastGroupKind::Tempo
                    } else {
                        TafForecastGroupKind::Inter
                    },
                    Some(from.to_string()),
                    Some(to.to_string()),
                )
            } else {
                let (from, to) = parse_validity_range(next)?;
                index += 1;
                (
                    TafForecastGroupKind::Prob,
                    Some(from.to_string()),
                    Some(to.to_string()),
                )
            };
            (
                change_kind,
                valid_from,
                valid_to,
                Some(probability_percent),
                tokens.len(),
            )
        } else {
            return None;
        };

    let body = tokens[index..body_end].join(" ");
    Some(TafForecastGroup {
        change_kind,
        valid_from,
        valid_to,
        probability_percent,
        conditions: parse_conditions(&tokens[index..body_end]),
        raw: body,
    })
}

fn parse_conditions(tokens: &[&str]) -> TafConditions {
    let mut conditions = TafConditions::default();

    for token in tokens {
        if conditions.wind.is_none()
            && let Some(wind) = parse_wind(token)
        {
            conditions.wind = Some(wind);
            continue;
        }
        if conditions.visibility.is_none() && is_visibility_token(token) {
            conditions.visibility = Some((*token).to_string());
            continue;
        }
        if conditions.wind_shear.is_none()
            && let Some(shear) = parse_wind_shear(token)
        {
            conditions.wind_shear = Some(shear);
            continue;
        }
        if let Some(sky) = parse_sky_condition(token) {
            conditions.sky_conditions.push(sky);
            continue;
        }
        if is_weather_token(token) {
            conditions.weather.push((*token).to_string());
        }
    }

    conditions
}

fn is_group_marker(token: &str) -> bool {
    token.starts_with("FM")
        || token == "TEMPO"
        || token == "BECMG"
        || token == "INTER"
        || token.starts_with("PROB30")
        || token.starts_with("PROB40")
}

fn parse_wind(token: &str) -> Option<TafWind> {
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
        if !direction.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
        (Some(direction.parse::<u16>().ok()?), rest, false)
    };
    let (speed, gust_kt) = if let Some((speed, gust)) = remainder.split_once('G') {
        (speed.parse::<u16>().ok()?, Some(gust.parse::<u16>().ok()?))
    } else {
        (remainder.parse::<u16>().ok()?, None)
    };
    Some(TafWind {
        direction_degrees: direction,
        speed_kt: speed,
        gust_kt,
        is_variable,
    })
}

fn is_visibility_token(token: &str) -> bool {
    token == "CAVOK"
        || token == "9999"
        || token.ends_with("SM")
        || token.starts_with('P') && token.ends_with("SM")
}

fn parse_wind_shear(token: &str) -> Option<TafWindShear> {
    let core = token.strip_prefix("WS")?;
    let (height, wind) = core.split_once('/')?;
    if height.len() != 3 {
        return None;
    }
    let height_ft_agl = height.parse::<u32>().ok()?.saturating_mul(100);
    let wind = wind.strip_suffix("KT")?;
    if wind.len() != 5 {
        return None;
    }
    Some(TafWindShear {
        height_ft_agl,
        direction_degrees: wind[..3].parse::<u16>().ok()?,
        speed_kt: wind[3..].parse::<u16>().ok()?,
    })
}

fn parse_sky_condition(token: &str) -> Option<TafSkyCondition> {
    let cover = token.get(..3)?;
    if !matches!(cover, "SKC" | "NSC" | "FEW" | "SCT" | "BKN" | "OVC" | "VV0") {
        return None;
    }
    let height_ft_agl = token
        .get(3..6)
        .filter(|digits| digits.chars().all(|ch| ch.is_ascii_digit()))
        .and_then(|digits| digits.parse::<u32>().ok())
        .map(|height| height * 100);
    Some(TafSkyCondition {
        cover: cover.to_string(),
        height_ft_agl,
    })
}

fn is_weather_token(token: &str) -> bool {
    let trimmed = token.trim_matches('+').trim_matches('-');
    trimmed.len() >= 2
        && trimmed.len() <= 8
        && trimmed.chars().all(|ch| ch.is_ascii_uppercase())
        && !matches!(trimmed, "RMK" | "AMD" | "COR" | "TX" | "TN" | "QNH")
        && !trimmed.starts_with("QNH")
}

fn next_token<'a>(input: &mut &'a str) -> Option<&'a str> {
    if input.is_empty() {
        return None;
    }

    if let Some((token, rest)) = input.split_once(' ') {
        *input = rest;
        Some(token)
    } else {
        let token = *input;
        *input = "";
        Some(token)
    }
}

fn is_station_token(token: &str) -> bool {
    (3..=4).contains(&token.len()) && token.chars().all(|ch| ch.is_ascii_alphanumeric())
}

fn is_issue_time_token(token: &str) -> bool {
    token.len() == 7 && token.ends_with('Z') && token[..6].chars().all(|ch| ch.is_ascii_digit())
}

fn parse_validity_range(token: &str) -> Option<(&str, &str)> {
    let (valid_from, valid_to) = token.split_once('/')?;
    (valid_from.len() == 4
        && valid_to.len() == 4
        && valid_from.chars().all(|ch| ch.is_ascii_digit())
        && valid_to.chars().all(|ch| ch.is_ascii_digit()))
    .then_some((valid_from, valid_to))
}

#[cfg(test)]
mod tests {
    use super::{TafForecastGroupKind, parse_taf_bulletin};

    #[test]
    fn parses_amended_taf_bulletin() {
        let text = "TAF AMD\nWBCF 070244Z 0703/0803 18012KT P6SM SCT050\n";
        let taf = parse_taf_bulletin(text).expect("expected TAF bulletin parsing to succeed");

        assert_eq!(taf.station, "WBCF");
        assert_eq!(taf.issue_time, "070244Z");
        assert_eq!(taf.valid_from.as_deref(), Some("0703"));
        assert_eq!(taf.valid_to.as_deref(), Some("0803"));
        assert!(taf.amendment);
        assert!(!taf.correction);
        assert_eq!(
            taf.initial_conditions
                .wind
                .as_ref()
                .map(|wind| wind.speed_kt),
            Some(12)
        );
    }

    #[test]
    fn parses_corrected_taf_bulletin() {
        let text = "TAF COR KBOS 090520Z 0906/1012 28012KT P6SM FEW250\n";
        let taf = parse_taf_bulletin(text).expect("expected TAF COR parsing to succeed");

        assert_eq!(taf.station, "KBOS");
        assert_eq!(taf.issue_time, "090520Z");
        assert!(taf.correction);
        assert!(!taf.amendment);
    }

    #[test]
    fn parses_bulletin_with_marker_line_before_taf_report() {
        let text = "TAF\nTAF SVJC 070400Z 0706/0806 07005KT 9999 FEW013 TX33/0718Z\n      TN23/0708Z\n      TEMPO 0706/0710 08004KT CAVOK\n     FM071100 09006KT 9999 FEW013=\n";
        let taf = parse_taf_bulletin(text).expect("expected TAF bulletin parsing to succeed");

        assert_eq!(taf.station, "SVJC");
        assert_eq!(taf.issue_time, "070400Z");
        assert_eq!(taf.valid_from.as_deref(), Some("0706"));
        assert_eq!(taf.valid_to.as_deref(), Some("0806"));
        assert_eq!(taf.groups.len(), 2);
        assert_eq!(taf.groups[0].change_kind, TafForecastGroupKind::Tempo);
        assert_eq!(taf.groups[1].change_kind, TafForecastGroupKind::Fm);
    }

    #[test]
    fn ignores_non_taf_body() {
        let text = "000 \nSAGL31 BGGH 070200\nMETAR BGKK 070220Z AUTO VRB02KT 9999NDV OVC043/// M03/M08 Q0967=\n";
        assert!(parse_taf_bulletin(text).is_none());
    }

    #[test]
    fn parses_probability_group() {
        let text =
            "TAF KDSM 090520Z 0906/1012 28012KT P6SM FEW250 PROB30 0910/0914 2SM TSRA BKN040CB";
        let taf = parse_taf_bulletin(text).expect("expected probability taf");

        assert_eq!(taf.groups.len(), 1);
        assert_eq!(taf.groups[0].change_kind, TafForecastGroupKind::Prob);
        assert_eq!(taf.groups[0].probability_percent, Some(30));
        assert!(
            taf.groups[0]
                .conditions
                .weather
                .contains(&"TSRA".to_string())
        );
    }

    #[test]
    fn parses_duplicated_amended_taf_prefix() {
        let text = "TAF AMD\nTAF AMD MMAS 090101Z 0901/0918 23008KT P6SM SCT100 BKN200\n";
        let taf = parse_taf_bulletin(text).expect("expected duplicated TAF AMD parsing to succeed");

        assert_eq!(taf.station, "MMAS");
        assert_eq!(taf.issue_time, "090101Z");
        assert_eq!(taf.valid_from.as_deref(), Some("0901"));
        assert_eq!(taf.valid_to.as_deref(), Some("0918"));
        assert!(taf.amendment);
        assert!(!taf.correction);
        assert!(taf.raw.starts_with("TAF AMD MMAS 090101Z"));
    }

    #[test]
    fn parses_station_led_taf_without_explicit_taf_prefix() {
        let text = "KPAM 061900Z 0619/0801 36009KT 9999 SCT030 QNH3007INS";
        let taf = parse_taf_bulletin(text).expect("expected station-led TAF parsing to succeed");

        assert_eq!(taf.station, "KPAM");
        assert_eq!(taf.issue_time, "061900Z");
        assert_eq!(taf.valid_from.as_deref(), Some("0619"));
        assert_eq!(taf.valid_to.as_deref(), Some("0801"));
    }

    #[test]
    fn parses_inter_group_and_trailing_probability_tempo() {
        let text = "TAF\nTAF YWLM 172013Z 1721/1818 17006KT 9999 -SHRA SCT010 BKN020\n     FM180200 15012KT 9999 -SHRA SCT012 BKN025\n      INTER 1721/1807 3000 SHRA BKN008 FEW025TCU\n      TEMPO 1807/1818 3000 SHRA BKN008 FEW025TCU PROB30=\n";
        let taf = parse_taf_bulletin(text).expect("expected INTER TAF parsing to succeed");

        assert_eq!(taf.station, "YWLM");
        assert_eq!(taf.groups.len(), 3);
        assert_eq!(taf.groups[0].change_kind, TafForecastGroupKind::Fm);
        assert_eq!(taf.groups[1].change_kind, TafForecastGroupKind::Inter);
        assert_eq!(taf.groups[1].valid_from.as_deref(), Some("1721"));
        assert_eq!(taf.groups[1].valid_to.as_deref(), Some("1807"));
        assert_eq!(taf.groups[2].change_kind, TafForecastGroupKind::Tempo);
        assert_eq!(taf.groups[2].probability_percent, Some(30));
        assert!(
            taf.groups[2]
                .conditions
                .weather
                .contains(&"SHRA".to_string())
        );
    }

    #[test]
    fn parses_probability_prefixed_tempo_group() {
        let text = "TAF EGDG 011206Z 0112/0212 04012KT 9999 FEW015 BKN040 PROB30 TEMPO 0200/0206 7000 HZ SCT010=";
        let taf = parse_taf_bulletin(text)
            .expect("expected probability-prefixed TEMPO parsing to succeed");

        assert_eq!(taf.groups.len(), 1);
        assert_eq!(taf.groups[0].change_kind, TafForecastGroupKind::Tempo);
        assert_eq!(taf.groups[0].probability_percent, Some(30));
        assert_eq!(taf.groups[0].valid_from.as_deref(), Some("0200"));
        assert_eq!(taf.groups[0].valid_to.as_deref(), Some("0206"));
    }
}
