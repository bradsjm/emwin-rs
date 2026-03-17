//! Product metadata lookup, hydrologic location lookup, and non-text filename classification.

mod generated_afos_routing;
mod generated_nwslid;
mod generated_text_products;
mod generated_ugc;
mod generated_wmo_office;
mod graphics;

use std::sync::OnceLock;

use crate::body::{BodyExtractionPlan, BodyExtractorId, body_extraction_plan};

pub use generated_nwslid::{NWSLID_ENTRY_COUNT, NWSLID_GENERATED_AT_UTC};
pub use generated_text_products::{TEXT_PRODUCT_ENTRY_COUNT, TEXT_PRODUCT_GENERATED_AT_UTC};
pub use generated_ugc::{
    UGC_COUNTY_ENTRY_COUNT, UGC_COUNTY_SOURCE_PATH, UGC_GENERATED_AT_UTC, UGC_ZONE_ENTRY_COUNT,
    UGC_ZONE_SOURCE_PATH,
};
pub use generated_wmo_office::{
    WMO_OFFICE_ENTRY_COUNT, WMO_OFFICE_GENERATED_AT_UTC, WMO_OFFICE_SOURCE_PATH,
};

/// Routing policy for AFOS text products in the generated catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextProductRouting {
    Generic,
    Cli,
    Fd,
    Pirep,
    Sigmet,
    Lsr,
    Cwa,
    Wwp,
    Cf6,
    Dsm,
    Hml,
    Mos,
    Saw,
    Sel,
    Mcd,
    Ero,
    SpcOutlook,
}

/// Generic body extraction policy for an AFOS text product.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TextProductBodyBehavior {
    Never,
    Catalog,
}

/// Metadata for a known AFOS text product catalog entry.
///
/// The catalog now drives both classification routing and generic body policy,
/// not just human-readable display metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct TextProductCatalogEntry {
    pub pil: &'static str,
    pub wmo_prefix: &'static str,
    pub title: &'static str,
    pub routing: TextProductRouting,
    pub body_behavior: TextProductBodyBehavior,
    /// Ordered generic body extractors derived from the product catalog.
    pub extractors: &'static [BodyExtractorId],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AfosRoutingOverride {
    pub(crate) afos: &'static str,
    pub(crate) title: Option<&'static str>,
    pub(crate) routing: Option<TextProductRouting>,
    pub(crate) body_behavior: Option<TextProductBodyBehavior>,
    pub(crate) extractors: Option<&'static [BodyExtractorId]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedTextProductPolicy {
    pub(crate) pil: &'static str,
    pub(crate) wmo_prefix: &'static str,
    pub(crate) title: &'static str,
    pub(crate) routing: TextProductRouting,
    pub(crate) body_behavior: TextProductBodyBehavior,
    pub(crate) extractors: &'static [BodyExtractorId],
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct NwslidEntry {
    pub nwslid: &'static str,
    pub state_code: &'static str,
    pub stream_name: &'static str,
    pub proximity: &'static str,
    pub place_name: &'static str,
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct UgcLocationEntry {
    pub code: &'static str,
    pub name: &'static str,
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct WmoOfficeEntry {
    pub code: &'static str,
    #[serde(skip_serializing)]
    pub office_name: &'static str,
    pub city: &'static str,
    pub state: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NonTextProductMeta {
    pub family: &'static str,
    pub title: &'static str,
    pub container: &'static str,
    pub pil: Option<&'static str>,
    pub wmo_prefix: Option<&'static str>,
}

/// Returns the catalog title for a three-character PIL prefix.
pub fn pil_description(nnn: &str) -> Option<&'static str> {
    text_product_catalog_entry(nnn).map(|entry| entry.title)
}

/// Looks up a hydrologic location entry by NWSLI code.
///
/// The lookup normalizes case before performing a binary search over the generated catalog.
pub fn nwslid_entry(code: &str) -> Option<&'static NwslidEntry> {
    let key = normalize_nwslid(code)?;
    generated_nwslid::NWSLID_CATALOG
        .binary_search_by_key(&key.as_str(), |entry| entry.nwslid)
        .ok()
        .map(|index| &generated_nwslid::NWSLID_CATALOG[index])
}

/// Looks up a county entry by UGC code.
///
/// The helper accepts either the canonical `XXCYYY` form or the same code without the middle
/// designator and normalizes before searching the generated catalog.
pub fn ugc_county_entry(code: &str) -> Option<&'static UgcLocationEntry> {
    let key = normalize_ugc(code, 'C')?;
    generated_ugc::UGC_COUNTY_CATALOG
        .binary_search_by_key(&key.as_str(), |entry| entry.code)
        .ok()
        .map(|index| &generated_ugc::UGC_COUNTY_CATALOG[index])
}

/// Looks up a forecast-zone entry by UGC code.
pub fn ugc_zone_entry(code: &str) -> Option<&'static UgcLocationEntry> {
    let key = normalize_ugc(code, 'Z')?;
    generated_ugc::UGC_ZONE_CATALOG
        .binary_search_by_key(&key.as_str(), |entry| entry.code)
        .ok()
        .map(|index| &generated_ugc::UGC_ZONE_CATALOG[index])
}

/// Looks up a WMO office entry by ICAO code.
///
/// Both three-letter and four-letter forms are accepted. Four-letter NWS identifiers are
/// normalized by stripping the leading `K` before the catalog search.
pub fn wmo_office_entry(code: &str) -> Option<&'static WmoOfficeEntry> {
    let key = normalize_wmo_office(code)?;
    generated_wmo_office::WMO_OFFICE_CATALOG
        .binary_search_by_key(&key.as_str(), |entry| entry.code)
        .ok()
        .map(|index| &generated_wmo_office::WMO_OFFICE_CATALOG[index])
}

/// Looks up the full text-product catalog entry for a PIL prefix.
pub fn text_product_catalog_entry(nnn: &str) -> Option<&'static TextProductCatalogEntry> {
    let key = normalize_pil(nnn)?;
    generated_text_products::TEXT_PRODUCT_CATALOG
        .binary_search_by_key(&key.as_str(), |entry| entry.pil)
        .ok()
        .map(|index| &generated_text_products::TEXT_PRODUCT_CATALOG[index])
}

pub(crate) fn resolved_text_product_policy(afos: &str) -> Option<ResolvedTextProductPolicy> {
    let pil = normalize_pil(afos.get(..3)?)?;
    let base = text_product_catalog_entry(&pil)?;
    let exact_afos = normalize_exact_afos(afos)?;

    let override_entry = generated_afos_routing::AFOS_ROUTING_OVERRIDES
        .binary_search_by_key(&exact_afos.as_str(), |entry| entry.afos)
        .ok()
        .map(|index| &generated_afos_routing::AFOS_ROUTING_OVERRIDES[index]);

    let body_behavior = override_entry
        .and_then(|entry| entry.body_behavior)
        .unwrap_or(base.body_behavior);
    let extractors = if matches!(body_behavior, TextProductBodyBehavior::Never) {
        &[]
    } else {
        override_entry
            .and_then(|entry| entry.extractors)
            .unwrap_or(base.extractors)
    };

    Some(ResolvedTextProductPolicy {
        pil: base.pil,
        wmo_prefix: base.wmo_prefix,
        title: override_entry
            .and_then(|entry| entry.title)
            .unwrap_or(base.title),
        routing: override_entry
            .and_then(|entry| entry.routing)
            .unwrap_or(base.routing),
        body_behavior,
        extractors,
    })
}

/// Returns the WMO `TTAAII` prefix associated with a PIL prefix.
pub fn wmo_prefix_for_pil(nnn: &str) -> Option<&'static str> {
    text_product_catalog_entry(nnn).map(|entry| entry.wmo_prefix)
}

/// Returns the internal body extraction plan for a text-product catalog entry.
pub(crate) fn body_extraction_plan_for_entry(
    entry: &TextProductCatalogEntry,
) -> Option<BodyExtractionPlan> {
    match entry.body_behavior {
        TextProductBodyBehavior::Never => None,
        TextProductBodyBehavior::Catalog => Some(body_extraction_plan(entry.extractors)),
    }
}

/// Classifies a non-text product filename into its family metadata.
pub(crate) fn classify_non_text_product(filename: &str) -> Option<NonTextProductMeta> {
    let canon = canonical_name(filename);
    let upper = canon.to_ascii_uppercase();

    graphics::detect_graphics(&upper)
}

fn normalize_pil(nnn: &str) -> Option<String> {
    let key = nnn.trim().to_ascii_uppercase();
    if key.len() == 3 { Some(key) } else { None }
}

fn normalize_exact_afos(afos: &str) -> Option<String> {
    let key = afos.trim().to_ascii_uppercase();
    if (3..=6).contains(&key.len()) && key.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        Some(key)
    } else {
        None
    }
}

fn normalize_nwslid(code: &str) -> Option<String> {
    let key = code.trim().to_ascii_uppercase();
    if key.len() == 5 && key.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        Some(key)
    } else {
        None
    }
}

fn normalize_ugc(code: &str, expected_class: char) -> Option<String> {
    let key = code.trim().to_ascii_uppercase();
    let bytes = key.as_bytes();
    if key.len() == 6
        && bytes[0].is_ascii_uppercase()
        && bytes[1].is_ascii_uppercase()
        && bytes[2] == expected_class as u8
        && bytes[3..].iter().all(u8::is_ascii_digit)
    {
        Some(key)
    } else {
        None
    }
}

fn normalize_wmo_office(code: &str) -> Option<String> {
    let key = code.trim().to_ascii_uppercase();
    match key.len() {
        3 if key.chars().all(|ch| ch.is_ascii_alphanumeric()) => Some(key),
        4 if key.chars().all(|ch| ch.is_ascii_alphanumeric()) => Some(key[1..].to_string()),
        _ => None,
    }
}

pub(crate) fn canonical_name(filename: &str) -> String {
    let base = filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(filename)
        .trim();
    if let Some((_, tail)) = base.rsplit_once('-') {
        let candidate = tail.trim();
        if candidate.contains('.')
            && !candidate.is_empty()
            && candidate
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        {
            return candidate.to_string();
        }
    }
    base.to_string()
}

pub(crate) fn container_from_ext(ext: &str) -> &'static str {
    if ext == "ZIP" { "zip" } else { "raw" }
}

pub(crate) fn container_from_filename(filename: &str) -> &'static str {
    let canon = canonical_name(filename);
    let ext = canon
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_uppercase())
        .unwrap_or_default();
    container_from_ext(&ext)
}

pub(super) fn radar_re() -> &'static regex::Regex {
    static RADAR_RE: OnceLock<regex::Regex> = OnceLock::new();
    RADAR_RE.get_or_init(|| regex::Regex::new(r"^(RAD[A-Z0-9]{5})\.(GIF)$").expect("valid regex"))
}

pub(super) fn goes_re() -> &'static regex::Regex {
    static GOES_RE: OnceLock<regex::Regex> = OnceLock::new();
    GOES_RE.get_or_init(|| {
        regex::Regex::new(r"^(G\d{2}[A-Z0-9]{6})\.(ZIP|JPG)$").expect("valid regex")
    })
}

pub(super) fn imgmod_re() -> &'static regex::Regex {
    static IMGMOD_RE: OnceLock<regex::Regex> = OnceLock::new();
    IMGMOD_RE.get_or_init(|| {
        regex::Regex::new(r"^((?:IMG|MOD)[A-Z0-9]{5})\.(ZIP|GIF|PNG|JPG)$").expect("valid regex")
    })
}

#[cfg(test)]
mod tests {
    use super::{
        TextProductBodyBehavior, TextProductRouting, classify_non_text_product, nwslid_entry,
        pil_description, resolved_text_product_policy, text_product_catalog_entry,
        ugc_county_entry, ugc_zone_entry, wmo_office_entry, wmo_prefix_for_pil,
    };
    use crate::body::BodyExtractorId;

    #[test]
    fn description_lookup_is_case_insensitive() {
        assert_eq!(pil_description("afd"), Some("Area Forecast Discussion"));
        assert_eq!(wmo_prefix_for_pil("AFD"), Some("FX"));
    }

    #[test]
    fn lookup_rejects_invalid_pil_length() {
        assert_eq!(text_product_catalog_entry("TO"), None);
        assert_eq!(pil_description("TOOO"), None);
    }

    #[test]
    fn detects_radar_graphic_family() {
        let meta = classify_non_text_product("RADUMSVY.GIF").expect("expected metadata");
        assert_eq!(meta.family, "radar_graphic");
        assert_eq!(meta.title, "Radar graphic");
        assert_eq!(meta.container, "raw");
    }

    #[test]
    fn detects_prefixed_filename() {
        let meta = classify_non_text_product("20260305-RADUMSVY.GIF").expect("expected metadata");
        assert_eq!(meta.family, "radar_graphic");
    }

    #[test]
    fn generated_catalog_contains_known_entry() {
        let entry = text_product_catalog_entry("ZFP").expect("expected generated catalog entry");
        assert_eq!(entry.title, "Zone Forecast Product");
        assert_eq!(entry.wmo_prefix, "FP");
        assert_eq!(entry.routing, TextProductRouting::Generic);
        assert_eq!(entry.body_behavior, TextProductBodyBehavior::Catalog);
        assert_eq!(entry.extractors, &[BodyExtractorId::Ugc]);
    }

    #[test]
    fn text_product_catalog_entry_returns_routing_and_body_behavior() {
        let entry =
            text_product_catalog_entry("SVR").expect("expected severe thunderstorm warning entry");
        assert_eq!(entry.routing, TextProductRouting::Generic);
        assert_eq!(entry.body_behavior, TextProductBodyBehavior::Catalog);
    }

    #[test]
    fn generated_catalog_exposes_ordered_extractors() {
        let entry = text_product_catalog_entry("FFW").expect("expected generated catalog entry");
        assert_eq!(entry.extractors, &[BodyExtractorId::VtecEvents]);
    }

    #[test]
    fn generic_warning_entry_exposes_catalog_body_behavior() {
        let entry =
            text_product_catalog_entry("SVR").expect("expected severe thunderstorm warning entry");
        assert_eq!(entry.routing, TextProductRouting::Generic);
        assert_eq!(entry.body_behavior, TextProductBodyBehavior::Catalog);
    }

    #[test]
    fn fd_entry_exposes_specialized_routing_and_never_body_behavior() {
        let entry = text_product_catalog_entry("FD1").expect("expected fd entry");
        assert_eq!(entry.routing, TextProductRouting::Fd);
        assert_eq!(entry.body_behavior, TextProductBodyBehavior::Never);
        assert!(entry.extractors.is_empty());
    }

    #[test]
    fn pirep_entry_exposes_specialized_routing_and_never_body_behavior() {
        let entry = text_product_catalog_entry("PIR").expect("expected pirep entry");
        assert_eq!(entry.routing, TextProductRouting::Pirep);
        assert_eq!(entry.body_behavior, TextProductBodyBehavior::Never);
        assert!(entry.extractors.is_empty());
    }

    #[test]
    fn sigmet_entry_exposes_specialized_routing_and_never_body_behavior() {
        let entry = text_product_catalog_entry("SIG").expect("expected sigmet entry");
        assert_eq!(entry.routing, TextProductRouting::Sigmet);
        assert_eq!(entry.body_behavior, TextProductBodyBehavior::Never);
        assert!(entry.extractors.is_empty());
    }

    #[test]
    fn new_specialized_entries_expose_specialized_routing_and_never_body_behavior() {
        for (pil, routing) in [
            ("ECS", TextProductRouting::Mos),
            ("LSR", TextProductRouting::Lsr),
            ("CWA", TextProductRouting::Cwa),
            ("WWP", TextProductRouting::Wwp),
            ("SAW", TextProductRouting::Saw),
            ("SEL", TextProductRouting::Sel),
            ("CF6", TextProductRouting::Cf6),
            ("DSM", TextProductRouting::Dsm),
            ("HML", TextProductRouting::Hml),
            ("LAV", TextProductRouting::Mos),
            ("LEV", TextProductRouting::Mos),
            ("MET", TextProductRouting::Mos),
            ("MAV", TextProductRouting::Mos),
            ("MEX", TextProductRouting::Mos),
            ("NBE", TextProductRouting::Mos),
            ("NBS", TextProductRouting::Mos),
            ("FRH", TextProductRouting::Mos),
            ("FTP", TextProductRouting::Mos),
            ("PTS", TextProductRouting::SpcOutlook),
        ] {
            let entry = text_product_catalog_entry(pil).expect("expected generated catalog entry");
            assert_eq!(entry.routing, routing);
            match pil {
                "SAW" => {
                    assert_eq!(entry.body_behavior, TextProductBodyBehavior::Catalog);
                    assert_eq!(
                        entry.extractors,
                        &[BodyExtractorId::Ugc, BodyExtractorId::LatLon]
                    );
                }
                "SEL" => {
                    assert_eq!(entry.body_behavior, TextProductBodyBehavior::Catalog);
                    assert_eq!(entry.extractors, &[BodyExtractorId::Ugc]);
                }
                _ => {
                    assert_eq!(entry.body_behavior, TextProductBodyBehavior::Never);
                    assert!(entry.extractors.is_empty());
                }
            }
        }
    }

    #[test]
    fn noisy_generic_ugc_families_can_disable_body_extraction() {
        for pil in ["FFG", "OPU", "REC", "RVM", "RWR", "SFT"] {
            let entry = text_product_catalog_entry(pil).expect("expected generated catalog entry");
            assert_eq!(entry.routing, TextProductRouting::Generic);
            assert_eq!(entry.body_behavior, TextProductBodyBehavior::Never);
            assert!(entry.extractors.is_empty());
        }
    }

    #[test]
    fn resolved_policy_defaults_to_base_catalog_entry() {
        let policy = resolved_text_product_policy("SAW0").expect("expected resolved policy");
        assert_eq!(policy.pil, "SAW");
        assert_eq!(policy.wmo_prefix, "WW");
        assert_eq!(policy.title, "Prelim Notice of Watch & Cancellation MSG");
        assert_eq!(policy.routing, TextProductRouting::Saw);
        assert_eq!(policy.body_behavior, TextProductBodyBehavior::Catalog);
        assert_eq!(
            policy.extractors,
            &[BodyExtractorId::Ugc, BodyExtractorId::LatLon]
        );
    }

    #[test]
    fn resolved_policy_applies_exact_afos_routing_overrides() {
        for (afos, title, routing) in [
            ("SWOMCD", "Mesoscale discussion", TextProductRouting::Mcd),
            (
                "FFGMPD",
                "Mesoscale precipitation discussion",
                TextProductRouting::Mcd,
            ),
            (
                "RBG94E",
                "Day 1 excessive rainfall outlook",
                TextProductRouting::Ero,
            ),
            (
                "PFWFD1",
                "Day 1 fire weather outlook points",
                TextProductRouting::SpcOutlook,
            ),
            ("NBXUSA", "NBM NBX Guidance", TextProductRouting::Mos),
            (
                "PRCUS",
                "State Pilot Report Collective",
                TextProductRouting::Pirep,
            ),
        ] {
            let policy = resolved_text_product_policy(afos).expect("expected resolved policy");
            assert_eq!(policy.title, title);
            assert_eq!(policy.routing, routing);
            assert_eq!(policy.body_behavior, TextProductBodyBehavior::Never);
            assert!(policy.extractors.is_empty());
        }
    }

    #[test]
    fn satellite_precipitation_estimates_keep_only_latlon_extraction() {
        let entry = text_product_catalog_entry("SPE").expect("expected generated catalog entry");
        assert_eq!(entry.routing, TextProductRouting::Generic);
        assert_eq!(entry.body_behavior, TextProductBodyBehavior::Catalog);
        assert_eq!(entry.extractors, &[BodyExtractorId::LatLon]);
    }

    #[test]
    fn nwslid_lookup_is_case_insensitive() {
        let entry = nwslid_entry("chfa2").expect("expected generated nwslid entry");
        assert_eq!(entry.nwslid, "CHFA2");
        assert_eq!(entry.state_code, "AK");
        assert_eq!(entry.stream_name, "Chena River");
        assert_eq!(entry.proximity, "at");
        assert_eq!(entry.place_name, "Fairbanks");
        assert_eq!(entry.latitude, 64.8458);
        assert_eq!(entry.longitude, -147.7011);
    }

    #[test]
    fn nwslid_lookup_rejects_invalid_codes() {
        assert!(nwslid_entry("TOO-LONG").is_none());
        assert!(nwslid_entry("AB!12").is_none());
        assert!(nwslid_entry("ZZZZZ").is_none());
    }

    #[test]
    fn ugc_county_lookup_is_case_insensitive() {
        let entry = ugc_county_entry("alc001").expect("expected generated county entry");
        assert_eq!(entry.code, "ALC001");
        assert_eq!(entry.name, "Autauga");
        assert_eq!(entry.latitude, 32.5349);
        assert_eq!(entry.longitude, -86.6428);
    }

    #[test]
    fn ugc_zone_lookup_is_case_insensitive() {
        let entry = ugc_zone_entry("akz317").expect("expected generated zone entry");
        assert_eq!(entry.code, "AKZ317");
        assert_eq!(entry.name, "City and Borough of Yakutat");
        assert_eq!(entry.latitude, 59.8909);
        assert_eq!(entry.longitude, -140.3727);
    }

    #[test]
    fn ugc_lookup_rejects_wrong_class() {
        assert!(ugc_county_entry("AKZ317").is_none());
        assert!(ugc_zone_entry("ALC001").is_none());
        assert!(ugc_zone_entry("BAD").is_none());
    }

    #[test]
    fn wmo_office_lookup_supports_three_and_four_letter_codes() {
        let entry = wmo_office_entry("LWX").expect("expected generated office entry");
        assert_eq!(entry.code, "LWX");
        assert_eq!(entry.office_name, "WFO Baltimore/Washington");
        assert_eq!(entry.city, "Baltimore/Washington");
        assert_eq!(entry.state, "DC");
        let json = serde_json::to_value(entry).expect("office entry serializes");
        assert!(json.get("office_name").is_none());

        let entry = wmo_office_entry("KLWX").expect("expected generated office entry");
        assert_eq!(entry.code, "LWX");
    }

    #[test]
    fn wmo_office_lookup_rejects_invalid_codes() {
        assert!(wmo_office_entry("XX").is_none());
        assert!(wmo_office_entry("ABCDE").is_none());
    }
}
