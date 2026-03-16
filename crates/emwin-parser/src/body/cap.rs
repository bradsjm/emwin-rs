//! CAP XML body projection into existing generic body shapes.

use crate::body::enrich::{
    BodyExtractionOutcome, BodyExtractionPlan, BodyExtractorId, GenericBody, ProductBody,
};
use crate::body::support::ascii_find_case_insensitive;
use crate::body::vtec_events::{VtecEventBody, VtecEventSegment, validate_vtec_event_segments};
use crate::{
    LatLonPolygon, ProductParseIssue, TimeMotLocEntry, UgcClass, UgcCode, UgcSection, VtecCode,
    WindHailEntry, WindHailKind, parse_vtec_codes_with_issues,
};
use chrono::{DateTime, Utc};
use quick_xml::Reader;
use quick_xml::events::Event;

#[derive(Debug, Default)]
struct CapAlert {
    infos: Vec<CapInfo>,
}

#[derive(Debug, Default, Clone)]
struct CapInfo {
    expires: Option<DateTime<Utc>>,
    parameters: Vec<CapValuePair>,
    areas: Vec<CapArea>,
}

#[derive(Debug, Default, Clone)]
struct CapArea {
    polygon: Option<String>,
    geocodes: Vec<CapValuePair>,
}

#[derive(Debug, Default, Clone)]
struct CapValuePair {
    name: String,
    value: String,
}

pub(crate) fn parse_cap_body_with_issues(
    text: &str,
    plan: &BodyExtractionPlan,
    reference_time: Option<DateTime<Utc>>,
) -> BodyExtractionOutcome {
    let Some(xml) = extract_cap_xml(text) else {
        return BodyExtractionOutcome {
            body: None,
            issues: vec![ProductParseIssue::new(
                "cap_parse",
                "missing_cap_xml",
                "could not locate CAP XML payload in the body text",
                None,
            )],
        };
    };

    let alert = match parse_cap_alert(&xml) {
        Ok(alert) => alert,
        Err(issue) => {
            return BodyExtractionOutcome {
                body: None,
                issues: vec![issue],
            };
        }
    };

    project_cap_alert(alert, plan, reference_time)
}

fn extract_cap_xml(text: &str) -> Option<String> {
    let start = ascii_find_case_insensitive(text, "<?xml")
        .or_else(|| ascii_find_case_insensitive(text, "<alert"))?;
    let end = text.rfind("</alert>")? + "</alert>".len();
    Some(text[start..end].trim().to_string())
}

fn parse_cap_alert(xml: &str) -> Result<CapAlert, ProductParseIssue> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut alert = CapAlert::default();
    let mut current_info: Option<CapInfo> = None;
    let mut current_area: Option<CapArea> = None;
    let mut current_value_name: Option<String> = None;
    let mut current_pair_value: Option<String> = None;
    let mut current_tag: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) => {
                let name = String::from_utf8_lossy(event.local_name().as_ref()).to_string();
                match name.as_str() {
                    "info" => current_info = Some(CapInfo::default()),
                    "area" => current_area = Some(CapArea::default()),
                    "parameter" | "geocode" => {
                        current_value_name = None;
                        current_pair_value = None;
                    }
                    "expires" | "polygon" | "valueName" | "value" => {
                        current_tag = Some(name);
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(text)) => {
                let value = String::from_utf8_lossy(text.as_ref()).trim().to_string();
                match current_tag.as_deref() {
                    Some("expires") => {
                        if let Some(info) = current_info.as_mut() {
                            info.expires = parse_cap_time(&value);
                        }
                    }
                    Some("polygon") => {
                        if let Some(area) = current_area.as_mut() {
                            area.polygon = (!value.is_empty()).then_some(value);
                        }
                    }
                    Some("valueName") => current_value_name = Some(value),
                    Some("value") => current_pair_value = Some(value),
                    _ => {}
                }
            }
            Ok(Event::End(event)) => {
                let name = String::from_utf8_lossy(event.local_name().as_ref()).to_string();
                match name.as_str() {
                    "valueName" | "value" | "expires" | "polygon" => current_tag = None,
                    "parameter" => {
                        if let (Some(info), Some(name), Some(value)) = (
                            current_info.as_mut(),
                            current_value_name.take(),
                            current_pair_value.take(),
                        ) {
                            info.parameters.push(CapValuePair { name, value });
                        }
                    }
                    "geocode" => {
                        if let (Some(area), Some(name), Some(value)) = (
                            current_area.as_mut(),
                            current_value_name.take(),
                            current_pair_value.take(),
                        ) {
                            area.geocodes.push(CapValuePair { name, value });
                        }
                    }
                    "area" => {
                        if let (Some(info), Some(area)) =
                            (current_info.as_mut(), current_area.take())
                        {
                            info.areas.push(area);
                        }
                    }
                    "info" => {
                        if let Some(info) = current_info.take() {
                            alert.infos.push(info);
                        }
                    }
                    "alert" => break,
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(ProductParseIssue::new(
                    "cap_parse",
                    "invalid_cap_xml",
                    format!("could not parse CAP XML payload: {error}"),
                    None,
                ));
            }
            _ => {}
        }
        buf.clear();
    }

    if alert.infos.is_empty() {
        return Err(ProductParseIssue::new(
            "cap_parse",
            "missing_cap_info",
            "CAP XML did not contain any info blocks",
            None,
        ));
    }

    Ok(alert)
}

fn project_cap_alert(
    alert: CapAlert,
    plan: &BodyExtractionPlan,
    reference_time: Option<DateTime<Utc>>,
) -> BodyExtractionOutcome {
    let mut issues = Vec::new();
    let mut vtec_segments = Vec::new();
    let mut generic = GenericBody::default();

    for info in alert.infos {
        let vtec_codes = project_vtec(&info, &mut issues);
        let time_mot_loc = project_time_mot_loc(&info, &mut issues);
        let wind_hail = project_wind_hail(&info, &mut issues);
        let expires = preferred_expiry(&info);

        let areas = if info.areas.is_empty() {
            vec![CapArea::default()]
        } else {
            info.areas.clone()
        };

        for area in areas {
            let ugc_sections = project_ugc(&area, expires, reference_time, &mut issues);
            let polygons = project_polygons(&area, &mut issues);

            if !vtec_codes.is_empty() && plan.extractors.contains(&BodyExtractorId::VtecEvents) {
                vtec_segments.push(VtecEventSegment {
                    segment_index: vtec_segments.len(),
                    vtec: vtec_codes.clone(),
                    ugc_sections,
                    hvtec: Vec::new(),
                    polygons,
                    time_mot_loc: time_mot_loc.clone(),
                    wind_hail: wind_hail.clone(),
                });
                continue;
            }

            if !ugc_sections.is_empty() && plan.extractors.contains(&BodyExtractorId::Ugc) {
                generic
                    .ugc
                    .get_or_insert_with(Vec::new)
                    .extend(ugc_sections);
            }
            if !polygons.is_empty() && plan.extractors.contains(&BodyExtractorId::LatLon) {
                generic.latlon.get_or_insert_with(Vec::new).extend(polygons);
            }
            if !time_mot_loc.is_empty() && plan.extractors.contains(&BodyExtractorId::TimeMotLoc) {
                generic
                    .time_mot_loc
                    .get_or_insert_with(Vec::new)
                    .extend(time_mot_loc.clone());
            }
            if !wind_hail.is_empty() && plan.extractors.contains(&BodyExtractorId::WindHail) {
                generic
                    .wind_hail
                    .get_or_insert_with(Vec::new)
                    .extend(wind_hail.clone());
            }
        }
    }

    let generic_has_content = generic.ugc.is_some()
        || generic.latlon.is_some()
        || generic.time_mot_loc.is_some()
        || generic.wind_hail.is_some();

    let body = if !vtec_segments.is_empty() {
        if generic_has_content {
            issues.push(ProductParseIssue::new(
                "cap_parse",
                "cap_mixed_projection_omitted",
                "CAP payload contained both VTEC and non-VTEC projected content; non-VTEC generic content was omitted to preserve the existing body contract",
                None,
            ));
        }
        issues.extend(validate_vtec_event_segments(&vtec_segments));
        Some(ProductBody::VtecEvent(VtecEventBody {
            segments: vtec_segments,
        }))
    } else if generic_has_content {
        Some(ProductBody::Generic(generic))
    } else {
        None
    };

    BodyExtractionOutcome { body, issues }
}

fn preferred_expiry(info: &CapInfo) -> Option<DateTime<Utc>> {
    parameter_value(info, "eventEndingTime")
        .and_then(parse_cap_time)
        .or(info.expires)
}

fn project_vtec(info: &CapInfo, issues: &mut Vec<ProductParseIssue>) -> Vec<VtecCode> {
    let mut codes = Vec::new();
    for pair in info.parameters.iter().filter(|pair| pair.name == "VTEC") {
        let (mut parsed, mut parse_issues) = parse_vtec_codes_with_issues(&pair.value);
        codes.append(&mut parsed);
        issues.append(&mut parse_issues);
    }
    codes
}

fn project_ugc(
    area: &CapArea,
    expires: Option<DateTime<Utc>>,
    reference_time: Option<DateTime<Utc>>,
    issues: &mut Vec<ProductParseIssue>,
) -> Vec<UgcSection> {
    let Some(expires) = expires.or(reference_time) else {
        if area.geocodes.iter().any(|pair| pair.name == "UGC") {
            issues.push(ProductParseIssue::new(
                "ugc_parse",
                "missing_reference_time",
                "could not project CAP UGC values because no expiry or reference time was available",
                None,
            ));
        }
        return Vec::new();
    };

    let mut codes = Vec::new();
    for value in area
        .geocodes
        .iter()
        .filter(|pair| pair.name == "UGC")
        .map(|pair| pair.value.trim())
    {
        match parse_cap_ugc_code(value) {
            Some(code) => codes.push(code),
            None => issues.push(ProductParseIssue::new(
                "ugc_parse",
                "invalid_ugc_codes",
                format!("could not parse CAP UGC value `{value}`"),
                Some(value.to_string()),
            )),
        }
    }

    if codes.is_empty() {
        Vec::new()
    } else {
        vec![UgcSection::from_codes(codes, expires)]
    }
}

fn parse_cap_ugc_code(value: &str) -> Option<UgcCode> {
    if value.len() != 6 {
        return None;
    }
    let state = value[..2].to_string();
    let geoclass = UgcClass::from_char(value.chars().nth(2)?);
    let number = value[3..].parse().ok()?;
    Some(UgcCode {
        state,
        geoclass,
        number,
    })
}

fn project_polygons(area: &CapArea, issues: &mut Vec<ProductParseIssue>) -> Vec<LatLonPolygon> {
    let Some(raw) = area.polygon.as_deref() else {
        return Vec::new();
    };
    let mut points = Vec::new();
    for token in raw.split_whitespace() {
        let Some((lat, lon)) = token.split_once(',') else {
            issues.push(ProductParseIssue::new(
                "latlon_parse",
                "invalid_latlon_coordinate_format",
                format!("could not parse CAP polygon coordinate `{token}`"),
                Some(raw.to_string()),
            ));
            return Vec::new();
        };
        let Some(lat) = lat.trim().parse::<f64>().ok() else {
            issues.push(ProductParseIssue::new(
                "latlon_parse",
                "invalid_latlon_latitude",
                format!("could not parse CAP polygon latitude `{lat}`"),
                Some(raw.to_string()),
            ));
            return Vec::new();
        };
        let Some(lon) = lon.trim().parse::<f64>().ok() else {
            issues.push(ProductParseIssue::new(
                "latlon_parse",
                "invalid_latlon_longitude",
                format!("could not parse CAP polygon longitude `{lon}`"),
                Some(raw.to_string()),
            ));
            return Vec::new();
        };
        points.push((lat, lon));
    }
    match LatLonPolygon::from_points(points) {
        Ok(polygon) => vec![polygon],
        Err(issue) => {
            issues.push(issue);
            Vec::new()
        }
    }
}

fn project_time_mot_loc(
    info: &CapInfo,
    issues: &mut Vec<ProductParseIssue>,
) -> Vec<TimeMotLocEntry> {
    let Some(raw) = parameter_value(info, "eventMotionDescription") else {
        return Vec::new();
    };
    match parse_cap_event_motion(raw) {
        Ok(entry) => vec![entry],
        Err(issue) => {
            issues.push(issue);
            Vec::new()
        }
    }
}

fn parse_cap_event_motion(raw: &str) -> Result<TimeMotLocEntry, ProductParseIssue> {
    let tokens: Vec<&str> = raw.split("...").collect();
    if tokens.len() < 5 {
        return Err(ProductParseIssue::new(
            "time_mot_loc_parse",
            "invalid_time_mot_loc_format",
            format!("could not parse CAP eventMotionDescription `{raw}`"),
            Some(raw.to_string()),
        ));
    }

    let time_utc = parse_cap_time(tokens[0]).ok_or_else(|| {
        ProductParseIssue::new(
            "time_mot_loc_parse",
            "invalid_time_mot_loc_time",
            format!("could not parse CAP event motion time from `{raw}`"),
            Some(raw.to_string()),
        )
    })?;
    let direction_degrees = tokens[2]
        .strip_suffix("DEG")
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| {
            ProductParseIssue::new(
                "time_mot_loc_parse",
                "invalid_time_mot_loc_direction",
                format!("could not parse CAP event motion direction from `{raw}`"),
                Some(raw.to_string()),
            )
        })?;
    let speed_kt = tokens[3]
        .strip_suffix("KT")
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| {
            ProductParseIssue::new(
                "time_mot_loc_parse",
                "invalid_time_mot_loc_speed",
                format!("could not parse CAP event motion speed from `{raw}`"),
                Some(raw.to_string()),
            )
        })?;

    let mut points = Vec::new();
    for token in tokens[4].split_whitespace() {
        let Some((lat, lon)) = token.split_once(',') else {
            return Err(ProductParseIssue::new(
                "time_mot_loc_parse",
                "invalid_time_mot_loc_coordinate_count",
                format!("could not parse CAP event motion coordinate `{token}`"),
                Some(raw.to_string()),
            ));
        };
        let Some(lat) = lat.parse::<f64>().ok() else {
            return Err(ProductParseIssue::new(
                "time_mot_loc_parse",
                "invalid_time_mot_loc_latitude",
                format!("could not parse CAP event motion latitude `{lat}`"),
                Some(raw.to_string()),
            ));
        };
        let Some(lon) = lon.parse::<f64>().ok() else {
            return Err(ProductParseIssue::new(
                "time_mot_loc_parse",
                "invalid_time_mot_loc_longitude",
                format!("could not parse CAP event motion longitude `{lon}`"),
                Some(raw.to_string()),
            ));
        };
        points.push((lat, lon));
    }

    Ok(TimeMotLocEntry::from_parts(
        time_utc,
        direction_degrees,
        speed_kt,
        points,
    ))
}

fn project_wind_hail(info: &CapInfo, issues: &mut Vec<ProductParseIssue>) -> Vec<WindHailEntry> {
    let mut entries = Vec::new();

    if parameter_value(info, "windThreat").is_some() {
        entries.push(WindHailEntry::new(
            WindHailKind::WindThreat,
            None,
            None,
            None,
        ));
    }
    if let Some(value) = parameter_value(info, "maxWindGust") {
        match parse_numeric_parameter(value, Some("MPH")) {
            Some((numeric, units)) => entries.push(WindHailEntry::new(
                WindHailKind::MaxWindGust,
                Some(numeric),
                Some(units),
                None,
            )),
            None => issues.push(ProductParseIssue::new(
                "wind_hail_parse",
                "invalid_wind_hail_wind_value",
                format!("could not parse CAP maxWindGust value `{value}`"),
                Some(value.to_string()),
            )),
        }
    }
    if parameter_value(info, "hailThreat").is_some() {
        entries.push(WindHailEntry::new(
            WindHailKind::HailThreat,
            None,
            None,
            None,
        ));
    }
    if let Some(value) = parameter_value(info, "maxHailSize") {
        match parse_numeric_parameter(value, Some("IN")) {
            Some((numeric, units)) => entries.push(WindHailEntry::new(
                WindHailKind::MaxHailSize,
                Some(numeric),
                Some(units),
                None,
            )),
            None => issues.push(ProductParseIssue::new(
                "wind_hail_parse",
                "invalid_wind_hail_hail_value",
                format!("could not parse CAP maxHailSize value `{value}`"),
                Some(value.to_string()),
            )),
        }
    }

    entries
}

fn parse_numeric_parameter(value: &str, default_units: Option<&str>) -> Option<(f64, String)> {
    let mut parts = value.split_whitespace();
    let first = parts.next()?;
    let numeric = first.parse::<f64>().ok()?;
    let units = parts
        .next()
        .map(|value| value.to_ascii_uppercase())
        .or_else(|| default_units.map(str::to_string))?;
    Some((numeric, units))
}

fn parameter_value<'a>(info: &'a CapInfo, name: &str) -> Option<&'a str> {
    info.parameters
        .iter()
        .find(|pair| pair.name == name)
        .map(|pair| pair.value.as_str())
}

fn parse_cap_time(value: &str) -> Option<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP_WARNING: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<alert xmlns="urn:oasis:names:tc:emergency:cap:1.2">
    <info>
        <expires>2026-03-15T22:00:00-05:00</expires>
        <parameter><valueName>eventMotionDescription</valueName><value>2026-03-16T02:03:00-00:00...storm...264DEG...59KT...37.02,-87.86 36.66,-87.83</value></parameter>
        <parameter><valueName>windThreat</valueName><value>RADAR INDICATED</value></parameter>
        <parameter><valueName>maxWindGust</valueName><value>60 MPH</value></parameter>
        <parameter><valueName>hailThreat</valueName><value>RADAR INDICATED</value></parameter>
        <parameter><valueName>maxHailSize</valueName><value>0.75</value></parameter>
        <parameter><valueName>VTEC</valueName><value>/O.NEW.KPAH.SV.W.0040.260316T0203Z-260316T0300Z/</value></parameter>
        <parameter><valueName>eventEndingTime</valueName><value>2026-03-15T22:00:00-05:00</value></parameter>
        <area>
            <polygon>36.64,-87.06 36.63,-87.85 36.94,-87.91 36.97,-87.77 36.99,-87.75 37.21,-87.04 36.64,-87.06</polygon>
            <geocode><valueName>UGC</valueName><value>KYC047</value></geocode>
            <geocode><valueName>UGC</valueName><value>KYC177</value></geocode>
            <geocode><valueName>UGC</valueName><value>KYC219</value></geocode>
            <geocode><valueName>UGC</valueName><value>KYC221</value></geocode>
        </area>
    </info>
</alert>"#;

    const CAP_KEEPALIVE: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<alert xmlns="urn:oasis:names:tc:emergency:cap:1.2">
    <info>
        <expires>2026-03-16T02:07:42-00:00</expires>
        <area>
            <geocode><valueName>UGC</valueName><value>MDC031</value></geocode>
        </area>
    </info>
</alert>"#;

    #[test]
    fn cap_warning_projects_to_vtec_body() {
        let plan = crate::body::body_extraction_plan(&[
            BodyExtractorId::VtecEvents,
            BodyExtractorId::Ugc,
            BodyExtractorId::LatLon,
            BodyExtractorId::TimeMotLoc,
            BodyExtractorId::WindHail,
        ]);

        let outcome = parse_cap_body_with_issues(CAP_WARNING, &plan, Some(Utc::now()));

        let body = outcome.body.expect("expected body");
        let vtec = body.as_vtec_event().expect("expected vtec body");
        assert_eq!(vtec.segments.len(), 1);
        assert_eq!(vtec.segments[0].vtec.len(), 1);
        assert_eq!(vtec.segments[0].ugc_sections.len(), 1);
        assert_eq!(vtec.segments[0].polygons.len(), 1);
        assert_eq!(vtec.segments[0].time_mot_loc.len(), 1);
        assert!(vtec.segments[0].wind_hail.len() >= 2);
    }

    #[test]
    fn cap_keepalive_projects_to_generic_body() {
        let plan = crate::body::body_extraction_plan(&[
            BodyExtractorId::VtecEvents,
            BodyExtractorId::Ugc,
            BodyExtractorId::LatLon,
            BodyExtractorId::TimeMotLoc,
            BodyExtractorId::WindHail,
        ]);

        let outcome = parse_cap_body_with_issues(CAP_KEEPALIVE, &plan, Some(Utc::now()));

        let body = outcome.body.expect("expected body");
        let generic = body.as_generic().expect("expected generic body");
        assert!(generic.ugc.is_some());
        assert!(generic.latlon.is_none());
    }

    #[test]
    fn invalid_event_motion_reports_issue() {
        let raw = "2026-03-16T02:03:00-00:00...storm...BAD...59KT...37.02,-87.86";
        let issue = parse_cap_event_motion(raw).expect_err("expected invalid motion");
        assert_eq!(issue.code, "invalid_time_mot_loc_direction");
    }
}
