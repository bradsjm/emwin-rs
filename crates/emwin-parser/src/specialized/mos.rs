//! Parsing for MOS guidance bulletins.

use chrono::{DateTime, Datelike, NaiveDate, TimeDelta, Timelike, Utc};
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MosBulletin {
    pub sections: Vec<MosSection>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MosSection {
    pub station: String,
    pub model: String,
    pub runtime: String,
    pub forecasts: Vec<MosForecastRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MosForecastRow {
    pub valid: String,
    pub values: BTreeMap<String, String>,
}

pub(crate) fn parse_mos_bulletin(text: &str, reference_time: DateTime<Utc>) -> Option<MosBulletin> {
    let normalized = strip_control_chars(text);
    if normalized
        .lines()
        .any(|line| line.trim_start().starts_with(".B "))
    {
        return parse_ftp_bulletin(&normalized, reference_time);
    }
    let sections = split_sections(&normalized)?
        .into_iter()
        .filter_map(|section| parse_section(&section))
        .collect::<Vec<_>>();
    (!sections.is_empty()).then_some(MosBulletin { sections })
}

fn split_sections(text: &str) -> Option<Vec<String>> {
    let mut sections = Vec::new();
    let mut current = Vec::new();
    for line in text.lines() {
        let sanitized = sanitize_section_line(line);
        if parse_section_header(sanitized).is_some() && !current.is_empty() {
            sections.push(current.join("\n"));
            current.clear();
        }
        current.push(sanitized);
    }
    if !current.is_empty() {
        sections.push(current.join("\n"));
    }
    (!sections.is_empty()).then_some(sections)
}

fn parse_section(section: &str) -> Option<MosSection> {
    let lines = section.lines().collect::<Vec<_>>();
    let header = parse_section_header(sanitize_section_line(lines.first()?))?;
    let station = header.station;
    let mut model = header.model;
    let mos_name = header.mos_name;
    if mos_name == "LAMP" {
        model = "LAV".to_string();
    } else if model == "GFSX" {
        model = "MEX".to_string();
    } else if model == "NBM" {
        model = mos_name.clone();
    }
    let runtime = format!(
        "{}-{:02}-{:02}T{}:00:00Z",
        header.year,
        header.month,
        header.day,
        &header.hhmm[..2]
    );
    let init = DateTime::parse_from_rfc3339(&runtime)
        .ok()?
        .with_timezone(&Utc);
    let (times, data_lines) = parse_hour_axis(&lines, &model, init)?;
    let mut forecasts: Vec<MosForecastRow> = times
        .iter()
        .map(|ts: &DateTime<Utc>| MosForecastRow {
            valid: ts.to_rfc3339(),
            values: BTreeMap::new(),
        })
        .collect::<Vec<_>>();
    for line in data_lines.iter().copied() {
        if line.len() < 3 {
            continue;
        }
        let name = remap_var(line[..3].trim().replace('/', "_"));
        if name.is_empty() {
            continue;
        }
        let values = line[3..].split_whitespace().collect::<Vec<_>>();
        for (idx, value) in values.iter().enumerate() {
            if let Some(forecast) = forecasts.get_mut(idx) {
                forecast.values.insert(name.clone(), (*value).to_string());
            }
        }
    }
    Some(MosSection {
        station,
        model,
        runtime,
        forecasts,
    })
}

fn parse_ftp_bulletin(text: &str, reference_time: DateTime<Utc>) -> Option<MosBulletin> {
    let lines = text.lines().collect::<Vec<_>>();
    let header_end = lines.iter().position(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty() && !trimmed.starts_with(".B")
    })?;
    let header_lines = &lines[..header_end];
    let data_lines = &lines[header_end..];
    let header_text = header_lines
        .iter()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join("");
    let header = ftp_header_re().captures(&header_text)?;
    let model = header.name("model")?.as_str().to_string();
    let runtime = format!(
        "{:04}-{:02}-{:02}T{:02}:00:00Z",
        reference_time.year(),
        header.name("dc_month")?.as_str().parse::<u8>().ok()?,
        header.name("dc_day")?.as_str().parse::<u8>().ok()?,
        header.name("dc_hour")?.as_str().parse::<u8>().ok()?,
    );
    let base_date = NaiveDate::from_ymd_opt(
        reference_time.year(),
        header.name("base_month")?.as_str().parse::<u32>().ok()?,
        header.name("base_day")?.as_str().parse::<u32>().ok()?,
    )?;
    let mut sections = Vec::new();
    for line in data_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('.') {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let Some(station) = parts.next() else {
            continue;
        };
        let Some(value_text) = parts.next() else {
            continue;
        };
        let station = station.to_string();
        let values = value_text.split('/').collect::<Vec<_>>();
        if values.len() < 2 || values.len() % 2 != 0 {
            continue;
        }
        let mut forecasts = Vec::new();
        for (idx, pair) in values.chunks(2).enumerate() {
            let valid = base_date
                .checked_add_signed(TimeDelta::days(idx as i64))?
                .and_hms_opt(0, 0, 0)?
                .and_utc()
                .to_rfc3339();
            let mut map = BTreeMap::new();
            map.insert("TAIFBX".to_string(), pair[0].to_string());
            map.insert("TAIFBN".to_string(), pair[1].to_string());
            forecasts.push(MosForecastRow { valid, values: map });
        }
        sections.push(MosSection {
            station,
            model: model.clone(),
            runtime: runtime.clone(),
            forecasts,
        });
    }
    (!sections.is_empty()).then_some(MosBulletin { sections })
}

fn parse_hour_axis<'a>(
    lines: &'a [&'a str],
    model: &str,
    init: DateTime<Utc>,
) -> Option<(Vec<DateTime<Utc>>, Vec<&'a str>)> {
    let axis = ["FHR", "HR", "UTC"].into_iter().find_map(|label| {
        lines
            .iter()
            .position(|line| line.trim_start().starts_with(label))
            .map(|index| (label, index))
    })?;
    let hours = lines[axis.1]
        .split(|ch: char| ch.is_ascii_whitespace() || ch == '|')
        .skip(1)
        .map(str::to_string)
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let mut times: Vec<DateTime<Utc>> = Vec::new();
    for (idx, hour) in hours.iter().enumerate() {
        let ts = if axis.0 != "UTC"
            && (model == "LAV" || matches!(model, "MEX" | "NBE" | "NBS" | "NBX"))
        {
            init + TimeDelta::hours(i64::from(hour.parse::<u32>().ok()?))
        } else if hour == "00" && idx > 0 {
            let prev: DateTime<Utc> = *times.last()?;
            prev + TimeDelta::hours(i64::from((24 - prev.hour()) % 24))
        } else {
            let mut ts = init;
            ts = ts.with_hour(hour.parse::<u32>().ok()?)?;
            if !times.is_empty() && ts <= *times.last()? {
                ts += TimeDelta::days(1);
            }
            ts
        };
        times.push(ts);
    }
    Some((times, lines[axis.1 + 1..].to_vec()))
}

fn strip_control_chars(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_ascii_control() || ch.is_ascii_whitespace())
        .collect()
}

fn sanitize_section_line(line: &str) -> &str {
    line.trim_start_matches(|ch: char| {
        !ch.is_ascii_alphanumeric() && !ch.is_ascii_whitespace() && ch != '.'
    })
    .trim()
}

fn remap_var(name: String) -> String {
    match name.as_str() {
        "X_N" => "N_X".to_string(),
        "WND" => "WSP".to_string(),
        "WGS" => "GST".to_string(),
        _ => name,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SectionHeader {
    station: String,
    model: String,
    mos_name: String,
    month: u8,
    day: u8,
    year: i32,
    hhmm: String,
}

fn parse_section_header(line: &str) -> Option<SectionHeader> {
    let tokens = line.split_whitespace().collect::<Vec<_>>();
    let station = tokens.first()?.to_string();
    let model = tokens.get(1)?.to_string();
    let (mos_index, mos_name) = match tokens.get(2).copied() {
        Some(version) if version.starts_with('V') => (3, *tokens.get(3)?),
        Some(name) => (2, name),
        None => return None,
    };
    if tokens.get(mos_index + 1).copied() != Some("GUIDANCE") {
        return None;
    }
    let date = *tokens.get(mos_index + 2)?;
    let hhmm = tokens.get(mos_index + 3)?.to_string();
    if tokens.get(mos_index + 4).copied() != Some("UTC") || hhmm.len() != 4 {
        return None;
    }
    let (month, day, year) = date
        .split('/')
        .collect::<Vec<_>>()
        .as_slice()
        .try_into()
        .ok()
        .and_then(|[month, day, year]: [&str; 3]| {
            Some((
                month.parse::<u8>().ok()?,
                day.parse::<u8>().ok()?,
                year.parse::<i32>().ok()?,
            ))
        })?;

    Some(SectionHeader {
        station,
        model,
        mos_name: mos_name.to_string(),
        month,
        day,
        year,
        hhmm,
    })
}

fn ftp_header_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^\.B\s+(?P<model>[A-Z0-9]{3,5})\s+(?P<base_month>\d{2})(?P<base_day>\d{2})\s+DH\d{2}/DC(?P<dc_month>\d{2})(?P<dc_day>\d{2})(?P<dc_hour>\d{2})(?:\d{2})?",
        )
        .expect("valid FTP MOS header regex")
    })
}

#[cfg(test)]
mod tests {
    use super::{
        parse_hour_axis, parse_mos_bulletin, parse_section, parse_section_header, split_sections,
        strip_control_chars,
    };
    use chrono::Utc;

    #[test]
    fn parses_local_standard_mos_section() {
        let text = "\
KBCK NAM MET GUIDANCE 03/10/2026 0000 UTC
HR 00 03 06
TMP 20 21 22
WND 05 06 07";
        let bulletin = parse_mos_bulletin(text, Utc::now()).expect("mos bulletin");
        assert_eq!(bulletin.sections.len(), 1);
        assert_eq!(bulletin.sections[0].station, "KBCK");
        assert!(bulletin.sections[0].forecasts[0].values.contains_key("TMP"));
        assert!(bulletin.sections[0].forecasts[0].values.contains_key("WSP"));
    }

    #[test]
    fn parses_local_ftp_mos_section() {
        let text = "\
.B FTP 0310 DH06/DC03100600
AHP 12/08/13/09";
        let bulletin = parse_mos_bulletin(text, Utc::now()).expect("ftp mos bulletin");
        assert!(!bulletin.sections.is_empty());
        assert_eq!(bulletin.sections[0].station, "AHP");
        assert!(
            bulletin.sections[0].forecasts[0]
                .values
                .contains_key("TAIFBX")
        );
    }

    #[test]
    fn parses_mexafg_fixture() {
        let text = "\
PAOR GFSX MEX GUIDANCE 03/10/2026 0000 UTC
UTC 00 06 12
TMP 10 12 14
WND 05 06 07";
        let normalized = strip_control_chars(text);
        let sections = split_sections(&normalized).expect("mexafg sections");
        let lines = sections[0].lines().collect::<Vec<_>>();
        let header = parse_section_header(lines[0]).expect("mexafg header");
        let (_, data_lines) = parse_hour_axis(&lines, "MEX", Utc::now()).expect("mexafg axis");
        let section = parse_section(&sections[0]).expect("mexafg section");
        let bulletin = parse_mos_bulletin(text, Utc::now()).expect("mexafg bulletin");

        assert_eq!(header.station, "PAOR");
        assert!(!data_lines.is_empty());
        assert_eq!(bulletin.sections.len(), 1);
        assert_eq!(section.station, "PAOR");
        assert_eq!(bulletin.sections[0].station, "PAOR");
        assert_eq!(bulletin.sections[0].model, "MEX");
        assert!(!bulletin.sections[0].forecasts.is_empty());
    }

    #[test]
    fn preserves_nbx_model_identity() {
        let text = "\
CWCD NBM V3.2 NBX GUIDANCE 07/22/2020 1400 UTC
UTC 00 12
FHR 202 214
TMP 83 61
WSP 09 06";
        let bulletin = parse_mos_bulletin(text, Utc::now()).expect("nbx bulletin");

        assert!(!bulletin.sections.is_empty());
        assert!(
            bulletin
                .sections
                .iter()
                .all(|section| section.model == "NBX")
        );
    }
}
