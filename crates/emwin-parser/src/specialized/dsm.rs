//! Parsing for Daily Summary Message collectives.

use chrono::{DateTime, Datelike, NaiveDate, NaiveTime, Utc};
use regex::Regex;
use serde::Serialize;
use std::sync::OnceLock;

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

pub(crate) fn parse_dsm_bulletin(text: &str, reference_time: DateTime<Utc>) -> Option<DsmBulletin> {
    let normalized = text
        .chars()
        .filter(|ch| !ch.is_ascii_control() || matches!(ch, '\n' | '\r'))
        .collect::<String>()
        .replace(['\r', '\n'], "");
    let mut summaries = Vec::new();
    for token in normalized
        .split('=')
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        if let Some(summary) = parse_summary(token, reference_time) {
            summaries.push(summary);
        }
    }
    (!summaries.is_empty()).then_some(DsmBulletin { summaries })
}

fn parse_summary(token: &str, reference_time: DateTime<Utc>) -> Option<DsmSummary> {
    let caps = dsm_re().captures(token)?;
    let station = caps.name("station")?.as_str().to_string();
    let month = caps.name("month")?.as_str().parse::<u32>().ok()?;
    let day = caps.name("day")?.as_str().parse::<u32>().ok()?;
    let year = infer_year(reference_time, month);
    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    let timestring = |name: &str| -> Option<String> {
        let token = caps.name(name)?.as_str();
        let time = NaiveTime::parse_from_str(token, "%H%M").ok()?;
        Some(date.and_time(time).and_utc().to_rfc3339())
    };
    let hourly = (1..=24)
        .map(|idx| precip_hundredths(caps.name(&format!("p{idx:02}"))?.as_str()))
        .collect::<Option<Vec<_>>>()?;
    Some(DsmSummary {
        station,
        date: date.to_string(),
        max_temp_f: number_i16(caps.name("high")?.as_str()),
        max_temp_time: time_or_missing(caps.name("hightime").map(|m| m.as_str()), date),
        min_temp_f: number_i16(caps.name("low")?.as_str()),
        min_temp_time: time_or_missing(caps.name("lowtime").map(|m| m.as_str()), date),
        coop_max_temp_f: number_i16(caps.name("coophigh")?.as_str()),
        coop_min_temp_f: number_i16(caps.name("cooplow")?.as_str()),
        min_sea_level_pressure_mb_tenths: number_i32(caps.name("minslp")?.as_str()),
        min_slp_time: time_or_missing(caps.name("slptime").map(|m| m.as_str()), date),
        precip_day_inches: precip_hundredths(caps.name("pday")?.as_str())?,
        hourly_precip_inches: hourly,
        avg_wind_mph: caps
            .name("avg_sped")
            .and_then(|value| number_f32(value.as_str())),
        max_wind_mph: number_f32_opt(caps.name("sped_max").map(|m| m.as_str())),
        max_wind_time: timestring("time_sped_max"),
        max_wind_dir_degrees: wind_dir(caps.name("drct_sped_max").map(|m| m.as_str())),
        max_gust_mph: number_f32_opt(caps.name("sped_gust_max").map(|m| m.as_str())),
        max_gust_time: timestring("time_sped_gust_max"),
        max_gust_dir_degrees: wind_dir(caps.name("drct_gust_max").map(|m| m.as_str())),
    })
}

fn infer_year(reference_time: DateTime<Utc>, month: u32) -> i32 {
    if month == 12 && reference_time.month() == 1 {
        reference_time.year() - 1
    } else {
        reference_time.year()
    }
}

fn dsm_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(
        r"^(?P<station>[A-Z][A-Z0-9]{3})\s+DS\s+(?:COR\s+)?(?:\d{4}\s+)?(?P<day>\d\d)/(?P<month>\d\d)\s+(?:(?P<highmiss>M)|(?P<high>-?\d+)(?P<hightime>\d{4}))/\s*(?:(?P<lowmiss>M)|(?P<low>-?\d+)(?P<lowtime>\d{4}))//\s*(?P<coophigh>-?\d+|M)/\s*(?P<cooplow>-?\d+|M)//(?P<minslp>M|[-0-9]{3,4})(?P<slptime>\d{3,4})?/(?P<pday>T|M|[0-9]{1,4})/(?P<p01>T|M|-|[0-9]{1,4})/(?P<p02>T|M|-|[0-9]{1,4})/(?P<p03>T|M|-|[0-9]{1,4})/(?P<p04>T|M|-|[0-9]{1,4})/(?P<p05>T|M|-|[0-9]{1,4})/(?P<p06>T|M|-|[0-9]{1,4})/(?P<p07>T|M|-|[0-9]{1,4})/(?P<p08>T|M|-|[0-9]{1,4})/(?P<p09>T|M|-|[0-9]{1,4})/(?P<p10>T|M|-|[0-9]{1,4})/(?P<p11>T|M|-|[0-9]{1,4})/(?P<p12>T|M|-|[0-9]{1,4})/(?P<p13>T|M|-|[0-9]{1,4})/(?P<p14>T|M|-|[0-9]{1,4})/(?P<p15>T|M|-|[0-9]{1,4})/(?P<p16>T|M|-|[0-9]{1,4})/(?P<p17>T|M|-|[0-9]{1,4})/(?P<p18>T|M|-|[0-9]{1,4})/(?P<p19>T|M|-|[0-9]{1,4})/(?P<p20>T|M|-|[0-9]{1,4})/(?P<p21>T|M|-|[0-9]{1,4})/(?P<p22>T|M|-|[0-9]{1,4})/(?P<p23>T|M|-|[0-9]{1,4})/(?P<p24>T|M|-|[0-9]{1,4})(?:/(?P<avg_sped>M|-|\d{2,3})?)?/(?:(?P<drct_sped_max>\d{2})(?P<sped_max>\d{2,3})(?P<time_sped_max>\d{4})/(?P<drct_gust_max>\d{2})(?P<sped_gust_max>\d{2,3})(?P<time_sped_gust_max>\d{4}))?(?:/(?:[A-Z0-9-]{1,4}|M|T))*/*$",
    ).expect("valid DSM regex"))
}

fn number_i16(value: &str) -> Option<i16> {
    if value == "M" {
        None
    } else {
        value.parse().ok()
    }
}

fn number_i32(value: &str) -> Option<i32> {
    if value == "M" {
        None
    } else {
        value.parse().ok()
    }
}

fn number_f32(value: &str) -> Option<f32> {
    if matches!(value, "M" | "-") {
        None
    } else {
        value.parse().ok()
    }
}

fn number_f32_opt(value: Option<&str>) -> Option<f32> {
    value.and_then(number_f32)
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
        let bulletin = parse_dsm_bulletin(text, Utc::now()).expect("dsm bulletin");
        assert_eq!(bulletin.summaries.len(), 1);
        assert_eq!(bulletin.summaries[0].station, "KGUP");
        assert_eq!(bulletin.summaries[0].hourly_precip_inches.len(), 24);
    }
}
