//! Body enrichment and parsing orchestration.
//!
//! Generic body parsing is driven by a data-derived extraction plan instead of
//! branching directly on per-product flag booleans. VTEC-bearing generic
//! products now emit an event-oriented body model, while non-VTEC generic
//! products continue to emit a simpler generic body shape.

use crate::body::cap::parse_cap_body_with_issues;
use crate::body::vtec_events::{
    VtecEventBody, parse_vtec_event_body_with_issues, vtec_event_body_has_marine_only_ugc,
    vtec_event_body_iter_location_points, vtec_event_body_iter_polygons,
};
use crate::{
    GeoBounds, GeoPoint, LatLonPolygon, ProductParseIssue, TimeMotLocEntry, UgcSection,
    WindHailEntry, parse_latlon_polygons_with_issues, parse_time_mot_loc_entries_with_issues,
    parse_ugc_sections_with_issues, parse_wind_hail_entries_with_issues,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};
use std::collections::BTreeSet;

/// Canonical parsed body representation for generic products.
#[derive(Debug, Clone, PartialEq)]
pub enum ProductBody {
    VtecEvent(VtecEventBody),
    Generic(GenericBody),
}

impl Serialize for ProductBody {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            ProductBody::VtecEvent(body) => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("kind", "vtec_event")?;
                map.serialize_entry("segments", &body.segments)?;
                map.end()
            }
            ProductBody::Generic(body) => {
                let mut len = 1;
                if body.ugc.is_some() {
                    len += 1;
                }
                if body.latlon.is_some() {
                    len += 1;
                }
                if body.time_mot_loc.is_some() {
                    len += 1;
                }
                if body.wind_hail.is_some() {
                    len += 1;
                }
                let mut map = serializer.serialize_map(Some(len))?;
                map.serialize_entry("kind", "generic")?;
                if let Some(ugc) = &body.ugc {
                    map.serialize_entry("ugc", ugc)?;
                }
                if let Some(latlon) = &body.latlon {
                    map.serialize_entry("latlon", latlon)?;
                }
                if let Some(time_mot_loc) = &body.time_mot_loc {
                    map.serialize_entry("time_mot_loc", time_mot_loc)?;
                }
                if let Some(wind_hail) = &body.wind_hail {
                    map.serialize_entry("wind_hail", wind_hail)?;
                }
                map.end()
            }
        }
    }
}

impl ProductBody {
    pub fn as_vtec_event(&self) -> Option<&VtecEventBody> {
        match self {
            ProductBody::VtecEvent(body) => Some(body),
            ProductBody::Generic(_) => None,
        }
    }

    pub fn as_generic(&self) -> Option<&GenericBody> {
        match self {
            ProductBody::VtecEvent(_) => None,
            ProductBody::Generic(body) => Some(body),
        }
    }

    pub fn iter_location_points(&self) -> impl Iterator<Item = GeoPoint> + '_ {
        match self {
            ProductBody::VtecEvent(body) => Box::new(vtec_event_body_iter_location_points(body))
                as Box<dyn Iterator<Item = GeoPoint> + '_>,
            ProductBody::Generic(body) => Box::new(generic_body_iter_location_points(body))
                as Box<dyn Iterator<Item = GeoPoint> + '_>,
        }
    }

    pub fn iter_polygons(&self) -> impl Iterator<Item = ParsedPolygon<'_>> + '_ {
        match self {
            ProductBody::VtecEvent(body) => Box::new(vtec_event_body_iter_polygons(body))
                as Box<dyn Iterator<Item = ParsedPolygon<'_>> + '_>,
            ProductBody::Generic(body) => Box::new(generic_body_iter_polygons(body))
                as Box<dyn Iterator<Item = ParsedPolygon<'_>> + '_>,
        }
    }
}

/// Parsed body representation for non-VTEC generic products.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct GenericBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ugc: Option<Vec<UgcSection>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latlon: Option<Vec<LatLonPolygon>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_mot_loc: Option<Vec<TimeMotLocEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wind_hail: Option<Vec<WindHailEntry>>,
}

fn generic_body_iter_location_points(body: &GenericBody) -> impl Iterator<Item = GeoPoint> + '_ {
    let time_mot_loc = body.time_mot_loc.iter().flat_map(|entries| {
        entries
            .iter()
            .flat_map(|entry| entry.points.iter().map(|&(lat, lon)| GeoPoint { lat, lon }))
    });
    let ugc = body.ugc.iter().flat_map(|sections| {
        sections.iter().flat_map(|section| {
            section
                .counties
                .values()
                .chain(section.zones.values())
                .chain(section.fire_zones.values())
                .chain(section.marine_zones.values())
                .flat_map(|areas| areas.iter())
                .filter_map(|area| {
                    area.lat
                        .zip(area.lon)
                        .map(|(lat, lon)| GeoPoint { lat, lon })
                })
        })
    });

    time_mot_loc.chain(ugc)
}

fn generic_body_iter_polygons(body: &GenericBody) -> impl Iterator<Item = ParsedPolygon<'_>> + '_ {
    body.latlon.iter().flat_map(|polygons| {
        polygons.iter().map(|polygon| ParsedPolygon {
            points: &polygon.points,
            bounds: crate::polygon_bounds(&polygon.points),
        })
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParsedPolygon<'a> {
    pub points: &'a [(f64, f64)],
    pub bounds: Option<GeoBounds>,
}

/// Stable identifier for a generic body extractor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyExtractorId {
    VtecEvents,
    Ugc,
    LatLon,
    TimeMotLoc,
    WindHail,
}

/// Stable identifier for post-parse body QC checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyQcRuleId {
    UgcDuplicateCode,
}

/// Ordered extractor and QC configuration derived from catalog metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BodyExtractionPlan {
    pub(crate) extractors: &'static [BodyExtractorId],
    pub(crate) qc_rules: &'static [BodyQcRuleId],
}

/// Input-format hint for plan-driven body parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyInputFormat {
    PlainText,
    CapXml,
}

/// Shared context passed to plan-driven extractors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BodyExtractionContext<'a> {
    pub(crate) text: &'a str,
    pub(crate) reference_time: Option<DateTime<Utc>>,
}

/// Final result produced by the plan-driven extraction engine.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct BodyExtractionOutcome {
    pub(crate) body: Option<ProductBody>,
    pub(crate) issues: Vec<ProductParseIssue>,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct BodyExtractionState {
    generic_body: GenericBody,
    vtec_event_body: Option<VtecEventBody>,
    issues: Vec<ProductParseIssue>,
    has_generic_content: bool,
}

const QC_NONE: &[BodyQcRuleId] = &[];
const QC_UGC_DUPLICATES: &[BodyQcRuleId] = &[BodyQcRuleId::UgcDuplicateCode];

/// Builds an extraction plan from an ordered extractor list.
pub(crate) fn body_extraction_plan(extractors: &'static [BodyExtractorId]) -> BodyExtractionPlan {
    let has_ugc = extractors.contains(&BodyExtractorId::Ugc);
    let qc_rules = if has_ugc { QC_UGC_DUPLICATES } else { QC_NONE };

    BodyExtractionPlan {
        extractors,
        qc_rules,
    }
}

/// Enriches text body content by deriving the extraction plan from the text-product catalog.
pub fn enrich_body(
    text: &str,
    pil: &str,
    reference_time: Option<DateTime<Utc>>,
) -> (Option<ProductBody>, Vec<ProductParseIssue>) {
    let Some(plan) = crate::data::text_product_catalog_entry(pil)
        .and_then(crate::data::body_extraction_plan_for_entry)
    else {
        return (None, Vec::new());
    };
    let outcome = enrich_body_from_plan(text, &plan, reference_time);
    (outcome.body, outcome.issues)
}

/// Runs the plan-driven extraction engine over a body text payload.
pub(crate) fn enrich_body_from_plan(
    text: &str,
    plan: &BodyExtractionPlan,
    reference_time: Option<DateTime<Utc>>,
) -> BodyExtractionOutcome {
    enrich_body_from_plan_with_format(text, plan, reference_time, BodyInputFormat::PlainText)
}

pub(crate) fn enrich_body_from_plan_with_format(
    text: &str,
    plan: &BodyExtractionPlan,
    reference_time: Option<DateTime<Utc>>,
    input_format: BodyInputFormat,
) -> BodyExtractionOutcome {
    if input_format == BodyInputFormat::CapXml {
        return parse_cap_body_with_issues(text, plan, reference_time);
    }

    let context = BodyExtractionContext {
        text,
        reference_time,
    };
    let mut state = BodyExtractionState::default();

    for extractor in plan.extractors {
        match extractor {
            BodyExtractorId::VtecEvents => apply_vtec_events_extractor(&mut state, &context),
            BodyExtractorId::Ugc => apply_ugc_extractor(&mut state, &context),
            BodyExtractorId::LatLon => apply_latlon_extractor(&mut state, &context),
            BodyExtractorId::TimeMotLoc => apply_time_mot_loc_extractor(&mut state, &context),
            BodyExtractorId::WindHail => apply_wind_hail_extractor(&mut state, &context),
        }
    }

    let body = if let Some(body) = state.vtec_event_body {
        Some(ProductBody::VtecEvent(body))
    } else if state.has_generic_content {
        Some(ProductBody::Generic(state.generic_body))
    } else {
        None
    };

    if let Some(body) = &body {
        state.issues.extend(validate_body_qc(body, plan));
    }

    BodyExtractionOutcome {
        body,
        issues: state.issues,
    }
}

fn apply_vtec_events_extractor(
    state: &mut BodyExtractionState,
    context: &BodyExtractionContext<'_>,
) {
    let (parsed, issues) = parse_vtec_event_body_with_issues(context.text, context.reference_time);
    if let Some(parsed) = parsed {
        state.vtec_event_body = Some(parsed);
    }
    state.issues.extend(issues);
}

fn apply_ugc_extractor(state: &mut BodyExtractionState, context: &BodyExtractionContext<'_>) {
    match context.reference_time {
        Some(reference_time) => {
            let (parsed, issues) = parse_ugc_sections_with_issues(context.text, reference_time);
            if !parsed.is_empty() {
                state.generic_body.ugc = Some(parsed);
                state.has_generic_content = true;
            }
            state.issues.extend(issues);
        }
        None => state.issues.push(ProductParseIssue::new(
            "ugc_parse",
            "missing_reference_time",
            "could not parse UGC sections because the header timestamp could not be resolved",
            None,
        )),
    }
}

fn apply_latlon_extractor(state: &mut BodyExtractionState, context: &BodyExtractionContext<'_>) {
    let (parsed, issues) = parse_latlon_polygons_with_issues(context.text);
    if !parsed.is_empty() {
        state.generic_body.latlon = Some(parsed);
        state.has_generic_content = true;
    }
    state.issues.extend(issues);
}

fn apply_time_mot_loc_extractor(
    state: &mut BodyExtractionState,
    context: &BodyExtractionContext<'_>,
) {
    match context.reference_time {
        Some(reference_time) => {
            let (parsed, issues) =
                parse_time_mot_loc_entries_with_issues(context.text, reference_time);
            if !parsed.is_empty() {
                state.generic_body.time_mot_loc = Some(parsed);
                state.has_generic_content = true;
            }
            state.issues.extend(issues);
        }
        None => state.issues.push(ProductParseIssue::new(
            "time_mot_loc_parse",
            "missing_reference_time",
            "could not parse TIME...MOT...LOC entries because the header timestamp could not be resolved",
            None,
        )),
    }
}

fn apply_wind_hail_extractor(state: &mut BodyExtractionState, context: &BodyExtractionContext<'_>) {
    let (parsed, issues) = parse_wind_hail_entries_with_issues(context.text);
    if !parsed.is_empty() {
        state.generic_body.wind_hail = Some(parsed);
        state.has_generic_content = true;
    }
    state.issues.extend(issues);
}

fn validate_body_qc(body: &ProductBody, plan: &BodyExtractionPlan) -> Vec<ProductParseIssue> {
    let mut issues = Vec::new();

    for rule in plan.qc_rules {
        match rule {
            BodyQcRuleId::UgcDuplicateCode => {
                let maybe_sections = match body {
                    ProductBody::VtecEvent(vtec_body) => {
                        if vtec_event_body_has_marine_only_ugc(vtec_body) {
                            None
                        } else {
                            vtec_body
                                .segments
                                .first()
                                .map(|segment| &segment.ugc_sections)
                        }
                    }
                    ProductBody::Generic(generic_body) => generic_body.ugc.as_ref(),
                };
                if let Some(ugc_sections) = maybe_sections {
                    let mut duplicates = BTreeSet::new();
                    for section in ugc_sections {
                        let mut seen = BTreeSet::new();
                        collect_duplicate_ugc_codes(
                            &section.counties,
                            'C',
                            &mut seen,
                            &mut duplicates,
                        );
                        collect_duplicate_ugc_codes(
                            &section.zones,
                            'Z',
                            &mut seen,
                            &mut duplicates,
                        );
                        collect_duplicate_ugc_codes(
                            &section.fire_zones,
                            'F',
                            &mut seen,
                            &mut duplicates,
                        );
                        collect_duplicate_ugc_codes(
                            &section.marine_zones,
                            'M',
                            &mut seen,
                            &mut duplicates,
                        );
                    }
                    if !duplicates.is_empty() {
                        issues.push(ProductParseIssue::new(
                            "body_qc",
                            "ugc_duplicate_code",
                            format!(
                                "encountered duplicated UGC codes in parsed product body: {}",
                                duplicates.into_iter().collect::<Vec<_>>().join(", ")
                            ),
                            None,
                        ));
                    }
                }
            }
        }
    }

    issues
}

fn collect_duplicate_ugc_codes(
    groups: &std::collections::BTreeMap<String, Vec<crate::UgcArea>>,
    class_code: char,
    seen: &mut BTreeSet<String>,
    duplicates: &mut BTreeSet<String>,
) {
    for (state, areas) in groups {
        for area in areas {
            let canonical = format!("{state}{class_code}{:03}", area.id);
            if !seen.insert(canonical.clone()) {
                duplicates.insert(canonical);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{BodyExtractorId, body_extraction_plan, enrich_body, enrich_body_from_plan};

    const VTEC_EVENTS_ONLY: &[BodyExtractorId] = &[BodyExtractorId::VtecEvents];
    const UGC_AND_TIME_MOT_LOC_EXTRACTORS: &[BodyExtractorId] =
        &[BodyExtractorId::Ugc, BodyExtractorId::TimeMotLoc];

    #[test]
    fn body_extraction_plan_maps_known_extractor_sets_to_expected_extractors() {
        let plan = body_extraction_plan(VTEC_EVENTS_ONLY);
        assert_eq!(plan.extractors, &[BodyExtractorId::VtecEvents]);
    }

    #[test]
    fn enrich_body_with_vtec_event_plan() {
        let text = r#"
000
WUUS53 KOAX 051200
FFWOAX

NEC001>003-051300-
/O.NEW.KOAX.SV.W.0001.250305T1200Z-250305T1800Z/
/MSRM1.3.ER.250305T1200Z.250305T1800Z.250306T0000Z.NO/

LAT...LON 4143 9613 4145 9610 4140 9608 4138 9612
TIME...MOT...LOC 1200Z 300DEG 25KT 4143 9613 4140 9608
HAILTHREAT...RADARINDICATED
MAXHAILSIZE...1.00 IN
WINDTHREAT...OBSERVED
MAXWINDGUST...60 MPH
"#;

        let outcome = enrich_body_from_plan(
            text,
            &body_extraction_plan(VTEC_EVENTS_ONLY),
            Some(Utc::now()),
        );
        let body = outcome.body.expect("expected body");
        let body = body.as_vtec_event().expect("expected vtec event body");
        assert_eq!(body.segments.len(), 1);
        assert_eq!(body.segments[0].vtec.len(), 1);
        assert_eq!(body.segments[0].ugc_sections.len(), 1);
        assert_eq!(body.segments[0].hvtec.len(), 1);
        assert_eq!(body.segments[0].polygons.len(), 1);
        assert_eq!(body.segments[0].time_mot_loc.len(), 1);
        assert_eq!(body.segments[0].wind_hail.len(), 4);
        assert!(outcome.issues.is_empty());
    }

    #[test]
    fn enrich_body_from_plan_preserves_current_extractor_order() {
        let plan = body_extraction_plan(UGC_AND_TIME_MOT_LOC_EXTRACTORS);
        let outcome = enrich_body_from_plan("plain text", &plan, None);

        assert_eq!(outcome.issues.len(), 2);
        assert_eq!(outcome.issues[0].kind, "ugc_parse");
        assert_eq!(outcome.issues[1].kind, "time_mot_loc_parse");
    }

    #[test]
    fn enrich_body_wrapper_matches_plan_based_engine() {
        let text = "/O.NEW.KOAX.SV.W.0001.250305T1200Z-250305T1800Z/";
        let plan = crate::data::text_product_catalog_entry("SVR")
            .and_then(crate::data::body_extraction_plan_for_entry)
            .expect("SVR should have body extraction plan");

        let wrapper = enrich_body(text, "SVR", Some(Utc::now()));
        let outcome = enrich_body_from_plan(text, &plan, Some(Utc::now()));

        assert_eq!(wrapper.0, outcome.body);
        assert_eq!(wrapper.1, outcome.issues);
    }
}
