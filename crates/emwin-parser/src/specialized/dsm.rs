//! Parsing for Daily Summary Message collectives.

use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, Utc};
use serde::Serialize;

use crate::ProductParseIssue;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DsmBulletin {
    pub summaries: Vec<DsmSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DsmSummary {
    pub station: String,
    pub date: String,
    pub max_temp_f: Option<i16>,
    pub max_temp_time: Option<String>,
    pub min_temp_f: Option<i16>,
    pub min_temp_time: Option<String>,
    pub coop_max_temp_f: Option<i16>,
    pub coop_min_temp_f: Option<i16>,
    pub min_sea_level_pressure_mb_tenths: Option<i32>,
    pub min_slp_time: Option<String>,
    pub precip_day_inches: Option<f32>,
    pub hourly_precip_inches: Vec<Option<f32>>,
    pub avg_wind_mph: Option<f32>,
    pub max_wind_mph: Option<f32>,
    pub max_wind_time: Option<String>,
    pub max_wind_dir_degrees: Option<u16>,
    pub max_gust_mph: Option<f32>,
    pub max_gust_time: Option<String>,
    pub max_gust_dir_degrees: Option<u16>,
}

pub(crate) fn parse_dsm_bulletin(
    text: &str,
    reference_time: DateTime<Utc>,
) -> Option<(DsmBulletin, Vec<ProductParseIssue>)> {
    let normalized = text
        .chars()
        .filter(|ch| !ch.is_ascii_control() || matches!(ch, '\n' | '\r'))
        .collect::<String>()
        .replace('\r', "");
    let mut summaries = Vec::new();
    let mut issues = Vec::new();

    for token in normalized
        .split('=')
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        match parse_summary(token, reference_time) {
            Some(summary) => summaries.push(summary),
            None if token
                .chars()
                .any(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit()) =>
            {
                issues.push(ProductParseIssue::new(
                    "dsm_parse",
                    "invalid_dsm_summary",
                    "could not parse DSM summary token",
                    Some(compact_ascii_whitespace(token)),
                ))
            }
            None => {}
        }
    }

    (!summaries.is_empty()).then_some((DsmBulletin { summaries }, issues))
}

fn parse_summary(token: &str, reference_time: DateTime<Utc>) -> Option<DsmSummary> {
    let compact = compact_ascii_whitespace(token);
    let mut parts = compact.split_whitespace();
    let station = parts.next()?;
    if !is_station(station) || parts.next()? != "DS" {
        return None;
    }
    let next = parts.next()?;
    let (month_day, remainder_tokens) = if is_collection_time(next) {
        (parts.next()?, parts.collect::<Vec<_>>())
    } else {
        (next, parts.collect::<Vec<_>>())
    };
    let (day, month) = parse_day_month(month_day)?;
    let year = infer_year(reference_time, month);
    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    let payload = remainder_tokens.join("");
    let fields = payload.split('/').map(str::trim).collect::<Vec<_>>();
    if fields.len() < 32 {
        return None;
    }

    let (max_temp_f, max_temp_time) = parse_temp_and_time(fields[0], date)?;
    let (min_temp_f, min_temp_time) = parse_temp_and_time(fields[1], date)?;
    let coop_max_temp_f = number_i16(required_field(&fields, 3)?)?;
    let coop_min_temp_f = number_i16(required_field(&fields, 4)?)?;
    let (min_sea_level_pressure_mb_tenths, min_slp_time) =
        parse_slp_and_time(required_field(&fields, 6)?, date)?;
    let precip_day_inches = precip_hundredths(required_field(&fields, 7)?)?;
    let hourly_precip_inches = (8..32)
        .map(|idx| precip_hundredths(required_field(&fields, idx)?))
        .collect::<Option<Vec<_>>>()?;

    let avg_wind_mph = fields.get(32).and_then(|value| number_f32(value));
    let (max_wind_dir_degrees, max_wind_mph, max_wind_time) = fields
        .get(33)
        .and_then(|value| parse_wind_triplet(value, date))
        .unwrap_or((None, None, None));
    let (max_gust_dir_degrees, max_gust_mph, max_gust_time) = fields
        .get(34)
        .and_then(|value| parse_wind_triplet(value, date))
        .unwrap_or((None, None, None));

    Some(DsmSummary {
        station: station.to_string(),
        date: date.to_string(),
        max_temp_f,
        max_temp_time,
        min_temp_f,
        min_temp_time,
        coop_max_temp_f,
        coop_min_temp_f,
        min_sea_level_pressure_mb_tenths,
        min_slp_time,
        precip_day_inches,
        hourly_precip_inches,
        avg_wind_mph,
        max_wind_mph,
        max_wind_time,
        max_wind_dir_degrees,
        max_gust_mph,
        max_gust_time,
        max_gust_dir_degrees,
    })
}

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

fn required_field<'a>(fields: &'a [&'a str], index: usize) -> Option<&'a str> {
    fields.get(index).copied()
}

fn is_station(token: &str) -> bool {
    token.len() == 4
        && token
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
}

fn is_collection_time(token: &str) -> bool {
    token.len() == 4 && token.chars().all(|ch| ch.is_ascii_digit())
}

fn parse_day_month(token: &str) -> Option<(u32, u32)> {
    let (day, month) = token.split_once('/')?;
    Some((day.parse().ok()?, month.parse().ok()?))
}

fn infer_year(reference_time: DateTime<Utc>, month: u32) -> i32 {
    if month == 12 && reference_time.month() == 1 {
        reference_time.year() - 1
    } else {
        reference_time.year()
    }
}

fn parse_temp_and_time(token: &str, date: NaiveDate) -> Option<(Option<i16>, Option<String>)> {
    if token == "M" {
        return Some((None, None));
    }
    if token.len() < 5 {
        return None;
    }
    let split_at = token.len().checked_sub(4)?;
    let value = token.get(..split_at)?;
    let time = token.get(split_at..)?;
    Some((number_i16(value)?, time_or_missing(Some(time), date)))
}

fn parse_slp_and_time(token: &str, date: NaiveDate) -> Option<(Option<i32>, Option<String>)> {
    if token == "M" {
        return Some((None, None));
    }
    if token.len() <= 4 {
        return Some((number_i32(token)?, None));
    }
    let split_at = token.len().checked_sub(4)?;
    let value = token.get(..split_at)?;
    let time = token.get(split_at..)?;
    Some((number_i32(value)?, time_or_missing(Some(time), date)))
}

fn parse_wind_triplet(
    token: &str,
    date: NaiveDate,
) -> Option<(Option<u16>, Option<f32>, Option<String>)> {
    let token = token.trim();
    if matches!(token, "" | "M" | "-" | "N" | "NN") {
        return Some((None, None, None));
    }
    if token.len() < 8 {
        return None;
    }
    let dir = token.get(..2)?;
    let time = token.get(token.len() - 4..)?;
    let speed = token.get(2..token.len() - 4)?;
    Some((
        wind_dir(Some(dir)),
        number_f32(speed),
        time_or_missing(Some(time), date),
    ))
}

fn number_i16(value: &str) -> Option<Option<i16>> {
    if value == "M" {
        Some(None)
    } else {
        value.parse().ok().map(Some)
    }
}

fn number_i32(value: &str) -> Option<Option<i32>> {
    if value == "M" {
        Some(None)
    } else {
        value.parse().ok().map(Some)
    }
}

fn number_f32(value: &str) -> Option<f32> {
    if matches!(value, "" | "M" | "-" | "N" | "NN") {
        None
    } else {
        value.parse().ok()
    }
}

fn precip_hundredths(value: &str) -> Option<Option<f32>> {
    match value {
        "M" => Some(None),
        "-" => Some(Some(0.0)),
        "T" => Some(Some(0.0)),
        _ => value.parse::<f32>().ok().map(|v| Some(v / 100.0)),
    }
}

fn wind_dir(value: Option<&str>) -> Option<u16> {
    value
        .and_then(|raw| raw.parse::<u16>().ok())
        .map(|v| v * 10)
}

fn time_or_missing(value: Option<&str>, date: NaiveDate) -> Option<String> {
    let token = value?;
    if token == "M" {
        return None;
    }
    let token = if token.len() == 3 {
        format!("0{token}")
    } else {
        token.to_string()
    };
    let time = NaiveTime::parse_from_str(&token, "%H%M").ok()?;
    Some(date.and_time(time).and_utc().to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::parse_dsm_bulletin;
    use chrono::Utc;

    #[test]
    fn parses_dsm_fixture() {
        let text = "KGUP DS 1700 09/03 671441/ 160639// 67/ 16//9861654/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/-/-/-/-/-/-/-/-/23211342/23291333=";
        let (bulletin, issues) = parse_dsm_bulletin(text, Utc::now()).expect("dsm bulletin");
        assert!(issues.is_empty());
        assert_eq!(bulletin.summaries.len(), 1);
        assert_eq!(bulletin.summaries[0].station, "KGUP");
        assert_eq!(bulletin.summaries[0].hourly_precip_inches.len(), 24);
    }

    #[test]
    fn parses_all_missing_dsm_summary() {
        let text = "KHKS DS 26/11 M/M//M/M//M/M/00/00/00/00/00/05/02/06/T/T/12/00/03/17/T/T/T/00/00/00/00/T/00/00/M/M/M/13/NN/N/N/NN/ET EP EW=";
        let (bulletin, issues) = parse_dsm_bulletin(text, Utc::now()).expect("dsm bulletin");
        assert!(issues.is_empty());
        assert_eq!(bulletin.summaries.len(), 1);
        assert_eq!(bulletin.summaries[0].station, "KHKS");
        assert_eq!(bulletin.summaries[0].hourly_precip_inches.len(), 24);
    }

    #[test]
    fn preserves_partial_success_with_invalid_summary_issue() {
        let text = "KXXX DS 26/11 M/M//M/M//M/M/00/00/00=BAD TOKEN=KGUP DS 1700 09/03 671441/160639//67/16//9861654/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/-/-/-/-/-/-/-/-/23211342/23291333=";
        let (bulletin, issues) = parse_dsm_bulletin(text, Utc::now()).expect("dsm bulletin");
        assert_eq!(bulletin.summaries.len(), 1);
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == "invalid_dsm_summary")
        );
    }
}
