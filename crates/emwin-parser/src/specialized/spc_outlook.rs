//! Parsing for SPC convective and fire weather outlook point products.

use crate::LatLonPolygon;
use regex::Regex;
use serde::Serialize;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpcOutlookKind {
    Convective,
    FireWeather,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpcOutlookFormat {
    Points,
    ArealOutline,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpcOutlookBulletin {
    pub product_kind: SpcOutlookKind,
    pub format: SpcOutlookFormat,
    pub days: Vec<SpcOutlookDay>,
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpcOutlookDay {
    pub day: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycle: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<String>,
    pub outlooks: Vec<SpcOutlookArea>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpcOutlookArea {
    pub category: String,
    pub threshold: String,
    pub polygons: Vec<LatLonPolygon>,
}

pub(crate) fn parse_spc_outlook_bulletin(
    text: &str,
    afos: Option<&str>,
) -> Option<SpcOutlookBulletin> {
    let normalized = text.replace('\r', "");
    let compact = normalized.trim().to_string();
    let afos = afos?;
    let product_kind = if afos.starts_with("PFW") {
        SpcOutlookKind::FireWeather
    } else {
        SpcOutlookKind::Convective
    };
    let days = day_numbers_for_afos(afos)?;
    let format = if compact.to_ascii_uppercase().contains("AREAL OUTLINE") {
        SpcOutlookFormat::ArealOutline
    } else {
        SpcOutlookFormat::Points
    };
    let (valid_from, valid_to) = valid_re()
        .captures(&compact)
        .map(|captures| {
            (
                captures
                    .name("from")
                    .map(|value| value.as_str().to_string()),
                captures.name("to").map(|value| value.as_str().to_string()),
            )
        })
        .unwrap_or((None, None));

    let outlooks = parse_outlook_areas(&compact);
    if outlooks.is_empty() && !matches!(format, SpcOutlookFormat::ArealOutline) {
        return None;
    }

    Some(SpcOutlookBulletin {
        product_kind,
        format,
        days: days
            .into_iter()
            .map(|day| SpcOutlookDay {
                day,
                cycle: None,
                valid_from: valid_from.clone(),
                valid_to: valid_to.clone(),
                outlooks: outlooks.clone(),
            })
            .collect(),
        raw: compact,
    })
}

fn day_numbers_for_afos(afos: &str) -> Option<Vec<u8>> {
    Some(match afos {
        "PTSDY1" | "PFWFD1" => vec![1],
        "PTSDY2" | "PFWFD2" => vec![2],
        "PTSDY3" => vec![3],
        "PTSD48" => vec![4, 5, 6, 7, 8],
        "PFWF38" => vec![3, 4, 5, 6, 7, 8],
        _ => return None,
    })
}

fn parse_outlook_areas(text: &str) -> Vec<SpcOutlookArea> {
    let lines: Vec<&str> = text.lines().collect();
    let mut areas = Vec::new();
    let mut category = None::<String>;
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim();
        if let Some(captures) = category_re().captures(line) {
            let Some(name) = captures.name("name") else {
                index += 1;
                continue;
            };
            category = Some(name.as_str().to_ascii_uppercase());
            index += 1;
            continue;
        }
        if line == "&&" {
            index += 1;
            continue;
        }
        if let Some(current_category) = category.clone() {
            let Some(captures) = threshold_re().captures(line) else {
                index += 1;
                continue;
            };
            let Some(threshold_match) = captures.name("threshold") else {
                index += 1;
                continue;
            };
            let threshold = threshold_match.as_str().to_ascii_uppercase();
            let mut raw_points = Vec::new();
            raw_points.push(line[threshold_match.end()..].trim().to_string());
            loop {
                index += 1;
                if index >= lines.len() || lines[index].trim() == "&&" {
                    break;
                }
                let peek = lines[index].trim();
                if threshold_re().is_match(peek) || category_re().is_match(peek) {
                    index -= 1;
                    break;
                }
                raw_points.push(peek.to_string());
            }
            let polygons = parse_polygon_tokens(&raw_points.join(" "));
            areas.push(SpcOutlookArea {
                category: current_category,
                threshold,
                polygons,
            });
        }
        index += 1;
    }
    areas
}

fn parse_polygon_tokens(raw_points: &str) -> Vec<LatLonPolygon> {
    let mut polygons = Vec::new();
    let mut current = Vec::new();
    for token in raw_points.split_whitespace() {
        if token == "99999999" {
            if let Some(polygon) = build_polygon(&current) {
                polygons.push(polygon);
            }
            current.clear();
            continue;
        }
        if token.len() == 8 && token.chars().all(|ch| ch.is_ascii_digit()) {
            current.push(token.to_string());
        }
    }
    if let Some(polygon) = build_polygon(&current) {
        polygons.push(polygon);
    }
    polygons
}

fn build_polygon(tokens: &[String]) -> Option<LatLonPolygon> {
    if tokens.len() < 3 {
        return None;
    }
    let mut points = Vec::with_capacity(tokens.len() + 1);
    for token in tokens {
        let lat = token[..4].parse::<f64>().ok()? / 100.0;
        let lon = -(token[4..].parse::<f64>().ok()? / 100.0);
        points.push((lat, lon));
    }
    if points.first() != points.last() {
        let first = *points.first()?;
        points.push(first);
    }
    Some(LatLonPolygon {
        wkt: format_polygon_wkt(&points),
        points,
    })
}

fn format_polygon_wkt(points: &[(f64, f64)]) -> String {
    let mut wkt = String::from("POLYGON((");
    for (index, (lat, lon)) in points.iter().enumerate() {
        if index > 0 {
            wkt.push_str(", ");
        }
        wkt.push_str(&format!("{lon:.6} {lat:.6}"));
    }
    wkt.push_str("))");
    wkt
}

fn valid_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?im)^VALID TIME\s+(?P<from>\d{6}Z)\s*-\s*(?P<to>\d{6}Z)$")
            .expect("valid spc valid regex")
    })
}

fn category_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^\.\.\.\s*(?P<name>[A-Z ]+?)\s*\.\.\.$").expect("valid spc category regex")
    })
}

fn threshold_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?P<threshold>[0-9]+\.[0-9]{2}|TSTM|MRGL|SLGT|ENH|MDT|HIGH|ELEV|CRIT|EXTM|IDRT|SDRT|CIG1|CIG2|CIG3)\b")
            .expect("valid spc threshold regex")
    })
}

#[cfg(test)]
mod tests {
    use super::{SpcOutlookFormat, SpcOutlookKind, parse_spc_outlook_bulletin};

    #[test]
    fn parses_convective_points_product() {
        let text = "\
VALID TIME 071300Z - 081200Z

... TORNADO ...

0.02 39419768 39819865 40749901 39419768
&&

... CATEGORICAL ...

MRGL 49061987 48451952 47761927 49061987";
        let bulletin =
            parse_spc_outlook_bulletin(text, Some("PTSDY1")).expect("spc outlook bulletin");
        assert_eq!(bulletin.product_kind, SpcOutlookKind::Convective);
        assert_eq!(bulletin.format, SpcOutlookFormat::Points);
        assert_eq!(bulletin.days.len(), 1);
        assert_eq!(bulletin.days[0].outlooks.len(), 2);
        assert_eq!(bulletin.days[0].outlooks[0].polygons.len(), 1);
    }

    #[test]
    fn parses_multi_day_product_ids() {
        let text = "\
VALID TIME 071300Z - 081200Z

... CATEGORICAL ...

MRGL 49061987 48451952 47761927 49061987";
        let bulletin =
            parse_spc_outlook_bulletin(text, Some("PTSD48")).expect("spc outlook bulletin");
        assert_eq!(
            bulletin.days.iter().map(|day| day.day).collect::<Vec<_>>(),
            vec![4, 5, 6, 7, 8]
        );
    }

    #[test]
    fn parses_areal_outline_without_point_geometry() {
        let text = "\
DAY 1 CONVECTIVE OUTLOOK AREAL OUTLINE
VALID TIME 071300Z - 081200Z

... TORNADO ...

&&
";
        let bulletin =
            parse_spc_outlook_bulletin(text, Some("PTSDY1")).expect("spc areal outline bulletin");

        assert_eq!(bulletin.format, SpcOutlookFormat::ArealOutline);
        assert_eq!(bulletin.days.len(), 1);
        assert!(bulletin.days[0].outlooks.is_empty());
    }
}
