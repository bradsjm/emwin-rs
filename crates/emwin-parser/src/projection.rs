//! External JSON projection types for product metadata.
//!
//! The parser keeps [`crate::ProductEnrichment`] as its canonical internal parse
//! result. This module projects that richer shape into stable v2 summary/detail
//! contracts used by server mode and persisted sidecars.

use crate::{
    BbbKind, HvtecCause, HvtecRecord, HvtecSeverity, ProductArtifact, ProductBody,
    ProductEnrichment, ProductEnrichmentSource, ProductParseIssue, UgcArea, UgcSection, VtecCode,
    WindHailEntry,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const SCHEMA_VERSION_V2: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProductOfficeV2 {
    pub code: &'static str,
    pub city: &'static str,
    pub state: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProductHeaderV2 {
    Afos {
        ttaaii: String,
        cccc: String,
        ddhhmm: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        bbb: Option<String>,
        afos: String,
    },
    Wmo {
        ttaaii: String,
        cccc: String,
        ddhhmm: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        bbb: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct ProductSummaryFacetsV2 {
    pub has_body: bool,
    pub has_artifact: bool,
    pub has_issues: bool,
    pub vtec_count: usize,
    pub ugc_count: usize,
    pub hvtec_count: usize,
    pub latlon_count: usize,
    pub time_mot_loc_count: usize,
    pub wind_hail_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct ProductSummaryKeysV2 {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ugc_codes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vtec_phenomena: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vtec_significance: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vtec_actions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vtec_offices: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub etns: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hvtec_nwslids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hvtec_causes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hvtec_severities: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hvtec_records: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct ProductSummaryIssuesV2 {
    pub count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProductSummaryV2 {
    pub schema_version: u8,
    pub source: ProductEnrichmentSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<&'static str>,
    pub container: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pil: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wmo_prefix: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbb_kind: Option<BbbKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub office: Option<ProductOfficeV2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<ProductHeaderV2>,
    pub facets: ProductSummaryFacetsV2,
    pub keys: ProductSummaryKeysV2,
    pub issues: ProductSummaryIssuesV2,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProductArtifactDetailV2 {
    pub kind: &'static str,
    pub data: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProductDetailV2 {
    pub schema_version: u8,
    pub source: ProductEnrichmentSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<&'static str>,
    pub container: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pil: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wmo_prefix: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbb_kind: Option<BbbKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub office: Option<ProductOfficeV2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<ProductHeaderV2>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<ProductBody>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact: Option<ProductArtifactDetailV2>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<ProductParseIssue>,
}

pub fn summarize_product_v2(product: &ProductEnrichment) -> ProductSummaryV2 {
    ProductSummaryV2 {
        schema_version: SCHEMA_VERSION_V2,
        source: product.source,
        family: product.family,
        artifact_kind: product.parsed.as_ref().map(ProductArtifact::kind),
        title: product.title,
        container: product.container,
        pil: product.pil.clone(),
        wmo_prefix: product.wmo_prefix,
        bbb_kind: product.bbb_kind,
        office: product.office.as_ref().map(project_office),
        header: project_header(product),
        facets: summary_facets(
            product.body.as_ref(),
            product.parsed.is_some(),
            &product.issues,
        ),
        keys: summary_keys(product.body.as_ref()),
        issues: summary_issues(&product.issues),
    }
}

pub fn detail_product_v2(product: &ProductEnrichment) -> ProductDetailV2 {
    ProductDetailV2 {
        schema_version: SCHEMA_VERSION_V2,
        source: product.source,
        family: product.family,
        artifact_kind: product.parsed.as_ref().map(ProductArtifact::kind),
        title: product.title,
        container: product.container,
        pil: product.pil.clone(),
        wmo_prefix: product.wmo_prefix,
        bbb_kind: product.bbb_kind,
        office: product.office.as_ref().map(project_office),
        header: project_header(product),
        body: product.body.clone(),
        artifact: product.parsed.as_ref().map(project_artifact_detail),
        issues: product.issues.clone(),
    }
}

fn project_office(office: &crate::WmoOfficeEntry) -> ProductOfficeV2 {
    ProductOfficeV2 {
        code: office.code,
        city: office.city,
        state: office.state,
    }
}

fn project_header(product: &ProductEnrichment) -> Option<ProductHeaderV2> {
    if let Some(header) = &product.header {
        return Some(ProductHeaderV2::Afos {
            ttaaii: header.ttaaii.clone(),
            cccc: header.cccc.clone(),
            ddhhmm: header.ddhhmm.clone(),
            bbb: header.bbb.clone(),
            afos: header.afos.clone(),
        });
    }

    product
        .wmo_header
        .as_ref()
        .map(|header| ProductHeaderV2::Wmo {
            ttaaii: header.ttaaii.clone(),
            cccc: header.cccc.clone(),
            ddhhmm: header.ddhhmm.clone(),
            bbb: header.bbb.clone(),
        })
}

fn project_artifact_detail(artifact: &ProductArtifact) -> ProductArtifactDetailV2 {
    let kind = artifact.kind();
    let mut data = artifact.detail_json();
    strip_raw_fields(&mut data);

    ProductArtifactDetailV2 { kind, data }
}

fn strip_raw_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("raw");
            for child in map.values_mut() {
                strip_raw_fields(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                strip_raw_fields(child);
            }
        }
        _ => {}
    }
}

fn summary_facets(
    body: Option<&ProductBody>,
    has_artifact: bool,
    issues: &[ProductParseIssue],
) -> ProductSummaryFacetsV2 {
    ProductSummaryFacetsV2 {
        has_body: body.is_some(),
        has_artifact,
        has_issues: !issues.is_empty(),
        vtec_count: body.map_or(0, body_vtec_count),
        ugc_count: body.map_or(0, body_ugc_count),
        hvtec_count: body.map_or(0, body_hvtec_count),
        latlon_count: body.map_or(0, body_latlon_count),
        time_mot_loc_count: body.map_or(0, body_time_mot_loc_count),
        wind_hail_count: body.map_or(0, body_wind_hail_count),
    }
}

fn summary_keys(body: Option<&ProductBody>) -> ProductSummaryKeysV2 {
    let Some(body) = body else {
        return ProductSummaryKeysV2::default();
    };

    let mut states = BTreeSet::new();
    let mut ugc_codes = BTreeSet::new();
    for section in body_ugc_sections(body) {
        collect_ugc_codes(&mut states, &mut ugc_codes, section, |s| &s.counties, 'C');
        collect_ugc_codes(&mut states, &mut ugc_codes, section, |s| &s.zones, 'Z');
        collect_ugc_codes(&mut states, &mut ugc_codes, section, |s| &s.fire_zones, 'F');
        collect_ugc_codes(
            &mut states,
            &mut ugc_codes,
            section,
            |s| &s.marine_zones,
            'Z',
        );
    }

    let mut vtec_phenomena = BTreeSet::new();
    let mut vtec_significance = BTreeSet::new();
    let mut vtec_actions = BTreeSet::new();
    let mut vtec_offices = BTreeSet::new();
    let mut etns = BTreeSet::new();
    for code in body_vtec_codes(body) {
        vtec_phenomena.insert(code.phenomena.clone());
        vtec_significance.insert(code.significance.to_string());
        vtec_actions.insert(code.action.clone());
        vtec_offices.insert(code.office.clone());
        etns.insert(code.etn);
    }

    let mut hvtec_nwslids = BTreeSet::new();
    let mut hvtec_causes = BTreeSet::new();
    let mut hvtec_severities = BTreeSet::new();
    let mut hvtec_records = BTreeSet::new();
    for code in body_hvtec_codes(body) {
        hvtec_nwslids.insert(code.nwslid.clone());
        hvtec_causes.insert(hvtec_cause_name(code.cause).to_string());
        hvtec_severities.insert(hvtec_severity_name(code.severity).to_string());
        hvtec_records.insert(hvtec_record_name(code.record).to_string());
    }

    ProductSummaryKeysV2 {
        states: states.into_iter().collect(),
        ugc_codes: ugc_codes.into_iter().collect(),
        vtec_phenomena: vtec_phenomena.into_iter().collect(),
        vtec_significance: vtec_significance.into_iter().collect(),
        vtec_actions: vtec_actions.into_iter().collect(),
        vtec_offices: vtec_offices.into_iter().collect(),
        etns: etns.into_iter().collect(),
        hvtec_nwslids: hvtec_nwslids.into_iter().collect(),
        hvtec_causes: hvtec_causes.into_iter().collect(),
        hvtec_severities: hvtec_severities.into_iter().collect(),
        hvtec_records: hvtec_records.into_iter().collect(),
    }
}

fn summary_issues(issues: &[ProductParseIssue]) -> ProductSummaryIssuesV2 {
    ProductSummaryIssuesV2 {
        count: issues.len(),
        codes: issues
            .iter()
            .map(|issue| issue.code.to_string())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
    }
}

fn body_ugc_sections(body: &ProductBody) -> Vec<&UgcSection> {
    match body {
        ProductBody::VtecEvent(body) => body
            .segments
            .iter()
            .flat_map(|segment| segment.ugc_sections.iter())
            .collect(),
        ProductBody::Generic(body) => body
            .ugc
            .iter()
            .flat_map(|sections| sections.iter())
            .collect(),
    }
}

fn body_vtec_codes(body: &ProductBody) -> Vec<&VtecCode> {
    match body {
        ProductBody::VtecEvent(body) => body
            .segments
            .iter()
            .flat_map(|segment| segment.vtec.iter())
            .collect(),
        ProductBody::Generic(_) => Vec::new(),
    }
}

fn body_hvtec_codes(body: &ProductBody) -> Vec<&crate::HvtecCode> {
    match body {
        ProductBody::VtecEvent(body) => body
            .segments
            .iter()
            .flat_map(|segment| segment.hvtec.iter())
            .collect(),
        ProductBody::Generic(_) => Vec::new(),
    }
}

fn body_wind_hail_entries(body: &ProductBody) -> Vec<&WindHailEntry> {
    match body {
        ProductBody::VtecEvent(body) => body
            .segments
            .iter()
            .flat_map(|segment| segment.wind_hail.iter())
            .collect(),
        ProductBody::Generic(body) => body
            .wind_hail
            .iter()
            .flat_map(|entries| entries.iter())
            .collect(),
    }
}

fn body_vtec_count(body: &ProductBody) -> usize {
    body_vtec_codes(body).len()
}

fn body_ugc_count(body: &ProductBody) -> usize {
    match body {
        ProductBody::VtecEvent(body) => body
            .segments
            .iter()
            .map(|segment| segment.ugc_sections.len())
            .sum(),
        ProductBody::Generic(body) => body.ugc.as_ref().map_or(0, Vec::len),
    }
}

fn body_hvtec_count(body: &ProductBody) -> usize {
    body_hvtec_codes(body).len()
}

fn body_latlon_count(body: &ProductBody) -> usize {
    match body {
        ProductBody::VtecEvent(body) => body
            .segments
            .iter()
            .map(|segment| segment.polygons.len())
            .sum(),
        ProductBody::Generic(body) => body.latlon.as_ref().map_or(0, Vec::len),
    }
}

fn body_time_mot_loc_count(body: &ProductBody) -> usize {
    match body {
        ProductBody::VtecEvent(body) => body
            .segments
            .iter()
            .map(|segment| segment.time_mot_loc.len())
            .sum(),
        ProductBody::Generic(body) => body.time_mot_loc.as_ref().map_or(0, Vec::len),
    }
}

fn body_wind_hail_count(body: &ProductBody) -> usize {
    body_wind_hail_entries(body).len()
}

fn collect_ugc_codes(
    states: &mut BTreeSet<String>,
    ugc_codes: &mut BTreeSet<String>,
    section: &UgcSection,
    select: fn(&UgcSection) -> &BTreeMap<String, Vec<UgcArea>>,
    class_code: char,
) {
    for (state, areas) in select(section) {
        states.insert(state.clone());
        for area in areas {
            ugc_codes.insert(format!("{state}{class_code}{:03}", area.id));
        }
    }
}

fn hvtec_severity_name(value: HvtecSeverity) -> &'static str {
    match value {
        HvtecSeverity::None => "none",
        HvtecSeverity::Minor => "minor",
        HvtecSeverity::Moderate => "moderate",
        HvtecSeverity::Major => "major",
        HvtecSeverity::Unknown => "unknown",
    }
}

fn hvtec_cause_name(value: HvtecCause) -> &'static str {
    match value {
        HvtecCause::DamFailure => "dam_failure",
        HvtecCause::ExcessiveRainfall => "excessive_rainfall",
        HvtecCause::GlacierOutburst => "glacier_outburst",
        HvtecCause::IceJam => "ice_jam",
        HvtecCause::RainAndSnowmelt => "rain_and_snowmelt",
        HvtecCause::Snowmelt => "snowmelt",
        HvtecCause::RainSnowmeltIceJam => "rain_snowmelt_ice_jam",
        HvtecCause::UpstreamFloodingStormSurge => "upstream_flooding_storm_surge",
        HvtecCause::UpstreamFloodingTidalEffects => "upstream_flooding_tidal_effects",
        HvtecCause::ElevatedUpstreamFlowTidalEffects => "elevated_upstream_flow_tidal_effects",
        HvtecCause::WindTidalEffects => "wind_tidal_effects",
        HvtecCause::UpstreamDamRelease => "upstream_dam_release",
        HvtecCause::MultipleCauses => "multiple_causes",
        HvtecCause::OtherEffects => "other_effects",
        HvtecCause::Other => "other",
        HvtecCause::Unknown => "unknown",
    }
}

fn hvtec_record_name(value: HvtecRecord) -> &'static str {
    match value {
        HvtecRecord::NearRecord => "near_record",
        HvtecRecord::NoRecord => "no_record",
        HvtecRecord::NotApplicable => "not_applicable",
        HvtecRecord::Unavailable => "unavailable",
        HvtecRecord::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::{detail_product_v2, summarize_product_v2};
    use crate::enrich_product;

    #[test]
    fn summary_uses_discriminated_header_and_compact_issues() {
        let product = enrich_product("AFDBOX.TXT", b"000 \nINVALID HEADER\nAFDBOX\nBody\n");
        let summary = summarize_product_v2(&product);
        let json = serde_json::to_value(&summary).expect("summary should serialize");

        assert_eq!(json["schema_version"], 2);
        assert!(json.get("artifact").is_none());
        assert_eq!(json["issues"]["count"], 1);
        assert_eq!(json["issues"]["codes"][0], "invalid_wmo_header");
        assert!(json.get("header").is_none());
    }

    #[test]
    fn detail_strips_raw_fields_from_artifact_payloads() {
        let product = enrich_product(
            "SAGL31.TXT",
            b"000 \nSAGL31 BGGH 070200\nMETAR BGKK 070220Z AUTO VRB02KT 9999NDV OVC043/// M03/M08 Q0967=\n",
        );
        let detail = detail_product_v2(&product);
        let json = serde_json::to_value(&detail).expect("detail should serialize");

        assert_eq!(json["artifact"]["kind"], "metar");
        assert!(json["artifact"]["data"]["reports"][0].get("raw").is_none());
    }
}
