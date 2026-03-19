//! Public product enrichment types and facade.
//!
//! The implementation now lives in the internal `pipeline` module. This file
//! retains the stable public result types and `enrich_product` entrypoint used
//! by downstream callers.

use crate::pipeline::{NormalizedInput, ParsedEnvelope, assemble_product_enrichment, classify};
use crate::{
    BbbKind, ProductBody, ProductParseIssue, TextProductHeader, WmoHeader, WmoOfficeEntry,
};
use serde::Serialize;

/// Source of product enrichment data.
///
/// Indicates how the product metadata was derived:
/// - Text products: parsed from WMO/AFOS headers
/// - Non-text products: classified from filename patterns
/// - Unknown: unable to determine product type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductEnrichmentSource {
    TextHeader,
    WmoBulletin,
    FilenameNonText,
    Unknown,
}

pub use crate::enrichment_artifact::ProductArtifact;

/// Enriched product metadata with classification, headers, and parsed content.
///
/// This struct contains all metadata extracted from a product, including
/// source classification, parsed headers, body elements (VTEC, UGC, polygons),
/// and any issues encountered during processing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProductEnrichment {
    /// How this enrichment was derived
    pub source: ProductEnrichmentSource,
    /// Product family classification (e.g., "nws_text_product", "metar_collective")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<&'static str>,
    /// Human-readable product title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<&'static str>,
    /// Container type ("raw", "zip")
    pub container: &'static str,
    /// Product Identifier Line (e.g., "SVR", "TOR")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pil: Option<String>,
    /// WMO header prefix (e.g., "WU", "WT")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wmo_prefix: Option<&'static str>,
    /// Originating WMO office information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub office: Option<WmoOfficeEntry>,
    /// Parsed text product header (AFOS)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<TextProductHeader>,
    /// Parsed WMO bulletin header
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wmo_header: Option<WmoHeader>,
    /// BBB amendment/correction type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbb_kind: Option<BbbKind>,
    /// Parsed body elements (VTEC, UGC, polygons, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<ProductBody>,
    /// Parsed specialized artifact when a structured parser matched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed: Option<ProductArtifact>,
    /// Issues encountered during parsing
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub issues: Vec<ProductParseIssue>,
}

/// Enriches a product by running the internal parsing pipeline.
///
/// The public API remains stable while the implementation is staged internally
/// as normalization, envelope construction, classification, and assembly.
pub fn enrich_product(filename: &str, bytes: &[u8]) -> ProductEnrichment {
    let normalized = NormalizedInput::from_input(filename, bytes);
    let raw_bytes = normalized.bytes.clone();
    let envelope = ParsedEnvelope::build(normalized);
    let outcome = classify(&envelope);

    assemble_product_enrichment(outcome, filename, &raw_bytes)
}

#[cfg(test)]
mod tests {
    use crate::{MetarBulletin, ProductArtifact};

    use super::{ProductEnrichmentSource, enrich_product};

    #[test]
    fn text_products_use_header_enrichment() {
        let enrichment =
            enrich_product("TAFPDKGA.TXT", b"000 \nFTUS42 KFFC 022320\nTAFPDK\nBody\n");

        assert_eq!(enrichment.source, ProductEnrichmentSource::TextHeader);
        assert_eq!(enrichment.pil.as_deref(), Some("TAF"));
        assert_eq!(enrichment.wmo_prefix, Some("FT"));
        assert_eq!(
            enrichment.office.as_ref().map(|office| office.code),
            Some("FFC")
        );
        assert_eq!(
            enrichment
                .header
                .as_ref()
                .map(|header| header.afos.as_str()),
            Some("TAFPDK")
        );
        assert!(enrichment.issues.is_empty());
        assert!(enrichment.wmo_header.is_none());
        assert!(enrichment.parsed.is_none());
        let json = serde_json::to_value(&enrichment).expect("enrichment serializes");
        assert!(json.get("flags").is_none());
    }

    #[test]
    fn text_products_do_not_fall_back_to_filename_heuristics() {
        let enrichment = enrich_product("TAFPDKGA.TXT", b"000 \nINVALID HEADER\nTAFPDK\nBody\n");

        assert_eq!(enrichment.source, ProductEnrichmentSource::TextHeader);
        assert_eq!(enrichment.family, Some("nws_text_product"));
        assert_eq!(enrichment.pil, None);
        assert_eq!(enrichment.issues.len(), 1);
        assert_eq!(enrichment.issues[0].code, "invalid_wmo_header");
        assert!(enrichment.wmo_header.is_none());
        assert!(enrichment.parsed.is_none());
        assert!(enrichment.office.is_none());
    }

    #[test]
    fn metar_collectives_use_wmo_fallback_without_afos() {
        let enrichment = enrich_product(
            "SAGL31.TXT",
            b"000 \nSAGL31 BGGH 070200\nMETAR BGKK 070220Z AUTO VRB02KT 9999NDV OVC043/// M03/M08 Q0967=\n",
        );

        assert_eq!(enrichment.source, ProductEnrichmentSource::WmoBulletin);
        assert_eq!(enrichment.family, Some("metar_collective"));
        assert_eq!(enrichment.title, Some("METAR bulletin"));
        assert_eq!(enrichment.pil, None);
        assert_eq!(enrichment.wmo_prefix, None);
        assert_eq!(
            enrichment
                .wmo_header
                .as_ref()
                .map(|header| header.ttaaii.as_str()),
            Some("SAGL31")
        );
        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_metar)
                .map(MetarBulletin::report_count),
            Some(1)
        );
        assert!(enrichment.office.is_none());
        assert!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_taf)
                .is_none()
        );
        assert!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_dcp)
                .is_none()
        );
        assert!(enrichment.issues.is_empty());
    }

    #[test]
    fn taf_bulletins_use_wmo_fallback_without_afos() {
        let enrichment = enrich_product(
            "TAFWBCFJ.TXT",
            b"000 \nFTXX01 KWBC 070200\nTAF AMD\nWBCF 070244Z 0703/0803 18012KT P6SM SCT050\n",
        );

        assert_eq!(enrichment.source, ProductEnrichmentSource::WmoBulletin);
        assert_eq!(enrichment.family, Some("taf_bulletin"));
        assert_eq!(enrichment.title, Some("Terminal Aerodrome Forecast"));
        assert_eq!(enrichment.pil, None);
        assert_eq!(
            enrichment
                .wmo_header
                .as_ref()
                .map(|header| header.ttaaii.as_str()),
            Some("FTXX01")
        );
        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_taf)
                .map(|taf| taf.station.as_str()),
            Some("WBCF")
        );
        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_taf)
                .map(|taf| taf.issue_time.as_str()),
            Some("070244Z")
        );
        assert_eq!(
            enrichment.office.as_ref().map(|office| office.code),
            Some("WBC")
        );
        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_taf)
                .map(|taf| (taf.valid_from.as_deref(), taf.valid_to.as_deref())),
            Some((Some("0703"), Some("0803")))
        );
        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_taf)
                .map(|taf| taf.amendment),
            Some(true)
        );
        assert!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_metar)
                .is_none()
        );
        assert!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_dcp)
                .is_none()
        );
        assert!(enrichment.issues.is_empty());
    }

    #[test]
    fn taf_bulletins_with_marker_line_before_report_use_wmo_fallback() {
        let enrichment = enrich_product(
            "TAFMD1.TXT",
            b"FTVN41 KWBC 070303\nTAF\nTAF SVJC 070400Z 0706/0806 07005KT 9999 FEW013 TX33/0718Z\n      TN23/0708Z\n      TEMPO 0706/0710 08004KT CAVOK\n     FM071100 09006KT 9999 FEW013=\n",
        );

        assert_eq!(enrichment.source, ProductEnrichmentSource::WmoBulletin);
        assert_eq!(enrichment.family, Some("taf_bulletin"));
        assert_eq!(
            enrichment
                .wmo_header
                .as_ref()
                .map(|header| header.ttaaii.as_str()),
            Some("FTVN41")
        );
        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_taf)
                .map(|taf| taf.station.as_str()),
            Some("SVJC")
        );
        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_taf)
                .map(|taf| taf.issue_time.as_str()),
            Some("070400Z")
        );
        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_taf)
                .map(|taf| (taf.valid_from.as_deref(), taf.valid_to.as_deref())),
            Some((Some("0706"), Some("0806")))
        );
        assert!(enrichment.issues.is_empty());
        assert!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_dcp)
                .is_none()
        );
    }

    #[test]
    fn dcp_bulletins_use_wmo_fallback_without_afos() {
        let enrichment = enrich_product(
            "MISDCPSV.TXT",
            b"SXMS50 KWAL 070258\n83786162 066025814\n16.23\n003\n137\n071\n088\n12.9\n137\n007\n00000\n 42-0NN  45E\n",
        );

        assert_eq!(enrichment.source, ProductEnrichmentSource::WmoBulletin);
        assert_eq!(enrichment.family, Some("dcp_telemetry_bulletin"));
        assert_eq!(enrichment.title, Some("GOES DCP telemetry bulletin"));
        assert_eq!(
            enrichment
                .wmo_header
                .as_ref()
                .map(|header| header.ttaaii.as_str()),
            Some("SXMS50")
        );
        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_dcp)
                .and_then(|bulletin| bulletin.platform_id.as_deref()),
            Some("83786162 066025814")
        );
        assert_eq!(
            enrichment.office.as_ref().map(|office| office.code),
            Some("WAL")
        );
        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_dcp)
                .map(|bulletin| bulletin.lines.len()),
            Some(11)
        );
        assert!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_metar)
                .is_none()
        );
        assert!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_taf)
                .is_none()
        );
        assert!(enrichment.issues.is_empty());
    }

    #[test]
    fn misa_bulletins_share_wallops_telemetry_fallback() {
        let enrichment = enrich_product(
            "MISA50US.TXT",
            b"SXPA50 KWAL 070309\n\x1eD6805150 066030901 \n05.06 \n008 \n180 \n056 \n098 \n12.8 \n183 \n018 \n00000 \n 39-0NN 141E\n",
        );

        assert_eq!(enrichment.source, ProductEnrichmentSource::WmoBulletin);
        assert_eq!(enrichment.family, Some("dcp_telemetry_bulletin"));
        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_dcp)
                .and_then(|bulletin| bulletin.platform_id.as_deref()),
            Some("D6805150 066030901")
        );
        assert_eq!(
            enrichment.office.as_ref().map(|office| office.code),
            Some("WAL")
        );
        assert!(enrichment.issues.is_empty());
    }

    #[test]
    fn misdcp_inline_telemetry_bulletins_share_wallops_telemetry_fallback() {
        let enrichment = enrich_product(
            "MISDCPNI.TXT",
            b"SXMN20 KWAL 070326\n2211F77E 066032650bB1F@VT@VT@VT@VT@VT@VT@VT@VT@VT@VT@VT@VT@Fx@Fx@Fx@Fx@Fx@Fx@Fx@Fx@Fx@Fx@Fx@Fx@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@Ta@TaJ 40+0NN  57E%\n",
        );

        assert_eq!(enrichment.source, ProductEnrichmentSource::WmoBulletin);
        assert_eq!(enrichment.family, Some("dcp_telemetry_bulletin"));
        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_dcp)
                .and_then(|bulletin| bulletin.platform_id.as_deref()),
            Some("2211F77E 066032650")
        );
        assert_eq!(
            enrichment.office.as_ref().map(|office| office.code),
            Some("WAL")
        );
        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_dcp)
                .map(|bulletin| bulletin.lines.len()),
            Some(1)
        );
        assert!(enrichment.issues.is_empty());
    }

    #[test]
    fn international_sigmet_bulletins_use_wmo_fallback_without_afos() {
        let enrichment = enrich_product(
            "WVID21.TXT",
            b"WVID21 WAAA 090100\nWAAF SIGMET 05 VALID 090100/090700 WAAA-\nWAAF UJUNG PANDANG  FIR VA ERUPTION MT IBU PSN N0129 E12738 VA CLD\nOBS AT 0040Z WI N0129 E12737 - N0131 E12738 - N0129 E12751 - N0117\nE12744 - N0129 E12737 SFC/FL070 MOV SE 10KT NC=\n",
        );

        assert_eq!(enrichment.source, ProductEnrichmentSource::WmoBulletin);
        assert_eq!(enrichment.family, Some("unsupported_wmo_bulletin"));
        assert_eq!(
            enrichment.issues.first().map(|issue| issue.code),
            Some("unsupported_international_sigmet_bulletin")
        );
        assert!(enrichment.parsed.is_none());
        assert!(enrichment.wmo_header.is_some());
    }

    #[test]
    fn international_pirep_bulletins_use_wmo_unsupported_source() {
        let enrichment = enrich_product(
            "PIREP.TXT",
            b"UAJP71 RJFF 171210\n\nPIREP MOD TURB OBSD AT 1210 SANOR F340 REPORTED BY A320\n",
        );

        assert_eq!(enrichment.source, ProductEnrichmentSource::WmoBulletin);
        assert_eq!(enrichment.family, Some("unsupported_wmo_bulletin"));
        assert_eq!(
            enrichment.issues.first().map(|issue| issue.code),
            Some("unsupported_international_pirep_bulletin")
        );
        assert!(enrichment.parsed.is_none());
        assert!(enrichment.wmo_header.is_some());
    }

    #[test]
    fn corrected_metar_bulletins_use_wmo_fallback_without_afos() {
        let enrichment = enrich_product(
            "SAGG31.TXT",
            b"SAGG31 UGTB 090030 CCA\nMETAR COR UGKO 090030Z 24007KT 9999 SCT030 BKN061 03/01 Q1029 NOSIG=\n",
        );

        assert_eq!(enrichment.source, ProductEnrichmentSource::WmoBulletin);
        assert_eq!(enrichment.family, Some("metar_collective"));
        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_metar)
                .map(MetarBulletin::report_count),
            Some(1)
        );
        assert!(enrichment.issues.is_empty());
    }

    #[test]
    fn duplicated_amended_taf_bulletins_use_wmo_fallback_without_afos() {
        let enrichment = enrich_product(
            "FTMX41.TXT",
            b"FTMX41 KWBC 090103 AAA\nTAF AMD\nTAF AMD MMAS 090101Z 0901/0918 23008KT P6SM SCT100 BKN200\n     FM091200 04005KT P6SM SCT200=\n",
        );

        assert_eq!(enrichment.source, ProductEnrichmentSource::WmoBulletin);
        assert_eq!(enrichment.family, Some("taf_bulletin"));
        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_taf)
                .map(|value| value.station.as_str()),
            Some("MMAS")
        );
        assert!(enrichment.issues.is_empty());
    }

    #[test]
    fn marker_line_then_corrected_taf_bulletins_use_wmo_fallback_without_afos() {
        let enrichment = enrich_product(
            "TAFMDCOR.TXT",
            b"FTXX60 KWBC 110130\nTAF\nTAF COR KSVN 110127Z 1101/1207 17006KT 9999 SKC QNH3008INS\n      BECMG 1117/1118 22009KT 9999 BKN060 QNH3004INS TX29/1117Z\n      TN17/1110Z=\n",
        );

        assert_eq!(enrichment.source, ProductEnrichmentSource::WmoBulletin);
        assert_eq!(enrichment.family, Some("taf_bulletin"));
        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_taf)
                .map(|value| value.station.as_str()),
            Some("KSVN")
        );
        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_taf)
                .map(|value| value.correction),
            Some(true)
        );
        assert!(enrichment.issues.is_empty());
    }

    #[test]
    fn wallops_telemetry_variants_with_symbol_noise_use_wmo_dcp_fallback() {
        for (filename, bytes, platform_id) in [
            (
                "MISA50US.TXT",
                b"SXPA50 KWAL 090055\nCE1107B6 068005524`BCT@Go@Gq@Gq@Gr@Gr@Gr@Gs@Gr@Gs@Gr@Gu@Gt~]w~\\T~^F~bF~d@~eS~gq~jl~l]~mo~sA~wyf 39+0NN  25E\n".as_slice(),
                "CE1107B6 068005524",
            ),
            (
                "MISDCPHN.TXT",
                b"SXHN40 KWAL 090038\n50423782 068003840bB1H_??_??_??_??_??_??_??_??@@@@@r@TaJ 47+0NN 175E\n".as_slice(),
                "50423782 068003840",
            ),
            (
                "MISDCPMG.TXT",
                b"SXMG40 KWAL 090050\n9650D70A 068005040\"A18.34B17.92C18.73D82.73E80.63F84.66G9.70H0.00I10.92J355.59K0.00L824.64M824.67N824.67O11.50P21.30Q0.11R-10.01S2360.16T0.00U1.20 38-0NN 397E\n".as_slice(),
                "9650D70A 068005040",
            ),
            (
                "MISDCPSV.TXT",
                b"SXMS50 KWAL 090100\n3B0190E2 068010020`@aW@ac@]C@aP@\\z@N\\B_G@Dn@]A@A_@FZ@\\~@@@@@@@TiFtd@aY@ae@\\g@aV@\\n@N_B_G@C{@\\h@AQ@Ek@\\i@@@@@@@TmFtd@a[@ai@\\Z@aW@\\\\@N\\B_F@DX@]W@AD@Ez@\\_@@@@@@@TsFtd@a\\@aj@\\L@aW@\\O@NYB_E@C^@]C@AO@Dz@\\U@@@@@B@TxFtd 38+0NN 145E\n".as_slice(),
                "3B0190E2 068010020",
            ),
        ] {
            let enrichment = enrich_product(filename, bytes);
            assert_eq!(enrichment.source, ProductEnrichmentSource::WmoBulletin);
            assert_eq!(enrichment.family, Some("dcp_telemetry_bulletin"));
            assert_eq!(
                enrichment
                    .parsed
                    .as_ref()
                    .and_then(ProductArtifact::as_dcp)
                    .and_then(|bulletin| bulletin.platform_id.as_deref()),
                Some(platform_id)
            );
            assert!(enrichment.issues.is_empty());
        }
    }

    #[test]
    fn unsupported_airmet_bulletins_use_wmo_unsupported_source() {
        let enrichment = enrich_product(
            "WAAB31.TXT",
            b"WAAB31 LATI 090038\nLAAA AIRMET 1 VALID 090100/090500 LATI-\nLAAA TIRANA FIR MOD ICE FCST S OF N4110 FL070/120 STNR NC=\n",
        );

        assert_eq!(enrichment.source, ProductEnrichmentSource::WmoBulletin);
        assert_eq!(enrichment.family, Some("unsupported_wmo_bulletin"));
        assert_eq!(enrichment.issues[0].code, "unsupported_airmet_bulletin");
        assert!(enrichment.wmo_header.is_some());
    }

    #[test]
    fn unsupported_canadian_bulletins_use_wmo_unsupported_source() {
        let enrichment = enrich_product(
            "FPCN11.TXT",
            b"FPCN11 CWWG 090059 AAD\nUPDATED FORECASTS FOR SOUTHERN MANITOBA ISSUED BY ENVIRONMENT CANADA\nAT 7:57 P.M. CDT SUNDAY 8 MARCH 2026 FOR TONIGHT MONDAY AND MONDAY\nNIGHT.\n",
        );

        assert_eq!(enrichment.source, ProductEnrichmentSource::WmoBulletin);
        assert_eq!(enrichment.family, Some("unsupported_wmo_bulletin"));
        assert_eq!(
            enrichment.issues[0].code,
            "unsupported_canadian_text_bulletin"
        );
        assert!(enrichment.wmo_header.is_some());
    }

    #[test]
    fn unsupported_surface_observation_bulletins_use_wmo_unsupported_source() {
        let enrichment = enrich_product(
            "SAHOURLY.TXT",
            b"SACN74 CWAO 090000 RRC\n\nNPL SA 0000 AUTO8 M M M 990/-36/-39/2703/M/     7003 61MM=\n",
        );

        assert_eq!(enrichment.source, ProductEnrichmentSource::WmoBulletin);
        assert_eq!(enrichment.family, Some("unsupported_wmo_bulletin"));
        assert_eq!(
            enrichment.issues[0].code,
            "unsupported_surface_observation_bulletin"
        );
        assert!(enrichment.wmo_header.is_some());
    }

    #[test]
    fn body_enrichment_uses_body_text_not_afos_line() {
        let enrichment = enrich_product(
            "RFDLWXVA.TXT",
            b"FNUS41 KLWX 070303\nRFDLWX\nVAZ507-508-071100-\n\nRangeland Fire Danger Forecast\nNational Weather Service Baltimore MD/Washington DC\n1003 PM EST Fri Mar 6 2026\n",
        );

        assert_eq!(enrichment.source, ProductEnrichmentSource::TextHeader);
        assert_eq!(enrichment.pil.as_deref(), Some("RFD"));
        assert!(enrichment.issues.is_empty());
        assert_eq!(
            enrichment
                .body
                .as_ref()
                .and_then(|body| body.as_generic())
                .and_then(|body| body.ugc.as_ref())
                .map(|sections| sections[0].zones["VA"]
                    .iter()
                    .map(|area| area.id)
                    .collect::<Vec<_>>()),
            Some(vec![507, 508])
        );
    }

    #[test]
    fn current_specialized_afos_products_remain_bodyless() {
        let fd = enrich_product(
            "FD1US1.TXT",
            b"000 \nFTUS80 KWBC 070000\nFD1US1\nDATA BASED ON 070000Z\nVALID 071200Z\nFT 3000 6000\nBOS 9900 2812\n",
        );
        assert!(
            fd.parsed
                .as_ref()
                .and_then(ProductArtifact::as_fd)
                .is_some()
        );
        assert!(fd.body.is_none());

        let pirep = enrich_product(
            "PIRXXX.TXT",
            b"000 \nUAUS01 KBOU 070000\nPIRBOU\nDEN UA /OV 35 SW /TM 1925 /FL050 /TP E145=\n",
        );
        assert!(
            pirep
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_pirep)
                .is_some()
        );
        assert!(pirep.body.is_none());

        let sigmet = enrich_product(
            "SIGABC.TXT",
            b"000 \nWSUS31 KKCI 070000\nSIGABC\nCONVECTIVE SIGMET 12C\nVALID UNTIL 2355Z\nIA MO\nFROM 20S DSM-30NW IRK\nAREA EMBD TS MOV FROM 24020KT.\n",
        );
        assert!(
            sigmet
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_sigmet)
                .is_some()
        );
        assert!(sigmet.body.is_none());

        let lsr = enrich_product(
            "LSRBMX.TXT",
            b"000 \nNWUS54 KBMX 100015\nLSRBMX\n..TIME...   ...EVENT...      ...CITY LOCATION...     ...LAT.LON...\n..DATE...   ....MAG....      ..COUNTY LOCATION..ST.. ...SOURCE....\n0150 AM     HAIL             BROOKSVILLE             34.40N 87.70W\n03/10/2026  1.00 IN          WINSTON             AL  PUBLIC\n&&\n",
        );
        assert!(
            lsr.parsed
                .as_ref()
                .and_then(ProductArtifact::as_lsr)
                .is_some()
        );
        assert!(lsr.body.is_none());

        let cwa = enrich_product(
            "CWAZLC.TXT",
            b"000 \nFAUS22 KZLC 100229\nCWAZLC\nZLC2 CWA 100230\nZLC CWA 202 VALID UNTIL 100630\nFROM 75W BIL-15NNE SHR-55SW DDY-45S OCS-35SSE SLC-75W BIL\nAREA MOD/ISO SEV MTN WAVE FL350-ABV FL450. ALTITUDE CHANGE OF +/-25KTS. RPRTD BY ACFT. VISIBLE ON SATELLITE. CWSU 100230Z. CO ID MT UT WY\n=\n",
        );
        assert!(
            cwa.parsed
                .as_ref()
                .and_then(ProductArtifact::as_cwa)
                .is_some()
        );
        assert!(cwa.body.is_none());

        let wwp = enrich_product(
            "WWP1.TXT",
            b"000 \nWWUS40 KWNS 102008\nWWP1\nTORNADO WATCH PROBABILITIES FOR WT 0031\nPROBABILITY TABLE:\nPROB OF 2 OR MORE TORNADOES : 20%\nPROB OF 1 OR MORE STRONG /EF2-EF5/ TORNADOES : 10%\nPROB OF 10 OR MORE SEVERE WIND EVENTS : 70%\nPROB OF 1 OR MORE WIND EVENTS >= 65 KNOTS : 40%\nPROB OF 10 OR MORE SEVERE HAIL EVENTS : 60%\nPROB OF 1 OR MORE HAIL EVENTS >= 2 INCHES : 30%\nPROB OF 6 OR MORE COMBINED SEVERE HAIL/WIND EVENTS : 95%\nATTRIBUTE TABLE:\nMAX HAIL /INCHES/ : 2.0\nMAX WIND GUSTS SURFACE /KNOTS/ : 70\nMAX TOPS /X 100 FEET/ : 500\nMEAN STORM MOTION VECTOR /DEGREES AND KNOTS/ : 24035\nPARTICULARLY DANGEROUS SITUATION : NO\n",
        );
        assert!(
            wwp.parsed
                .as_ref()
                .and_then(ProductArtifact::as_wwp)
                .is_some()
        );
        assert!(wwp.body.is_none());

        let cf6 = enrich_product(
            "CF6GSN.TXT",
            b"000 \nCXGM50 PGUM 100030\nCF6GSN\nPRELIMINARY LOCAL CLIMATOLOGICAL DATA\nSTATION: TEST STATION\nMONTH: MARCH\nYEAR: 2026\nDY MAX MIN AVG DEP HDD CDD PCP SNW SND AWD MWD DIR MIN PSBL SKY WX GST GDR\n 1 70 50 60 0 5 0 0.10 0.0 0 8.5 20 180 600 720 CLR RA 30 190\n",
        );
        assert!(
            cf6.parsed
                .as_ref()
                .and_then(ProductArtifact::as_cf6)
                .is_some()
        );
        assert!(cf6.body.is_none());

        let dsm = enrich_product(
            "DSMCQC.TXT",
            b"000 \nCXUS45 KABQ 110415\nDSMCQC\nKCQC DS 2100 10/03 631553/ 400627// 63/ 40//9671608/T/00/00/00/T/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/-/-/-/-/28282059/29431531\n",
        );
        assert!(
            dsm.parsed
                .as_ref()
                .and_then(ProductArtifact::as_dsm)
                .is_some()
        );
        assert!(dsm.body.is_none());

        let hml = enrich_product(
            "HMLMTR.TXT",
            br#"000 
SRUS56 KMTR 100002
HMLMTR
<?xml version="1.0"?>
<site id="AAMC1" name="ARROYO SECO" originator="MTR" generationtime="2026-03-10T00:02:00Z">
  <observed issued="2026-03-10T00:00:00Z" primaryName="Stage" primaryUnits="FT">
    <datum><valid>2026-03-10T00:00:00Z</valid><primary>2.5</primary></datum>
  </observed>
</site>
"#,
        );
        assert!(
            hml.parsed
                .as_ref()
                .and_then(ProductArtifact::as_hml)
                .is_some()
        );
        assert!(hml.body.is_none());

        let mos = enrich_product(
            "METBCK.TXT",
            b"000 \nFOUS46 KWNO 100000\nMETBCK\nKBCK NAM MET GUIDANCE 03/10/2026 0000 UTC\nHR 00 03 06\nTMP 20 21 22\nWND 05 06 07\n",
        );
        assert!(
            mos.parsed
                .as_ref()
                .and_then(ProductArtifact::as_mos)
                .is_some()
        );
        assert!(mos.body.is_none());
    }

    #[test]
    fn swomcd_products_route_to_structured_mcd_enrichment() {
        let enrichment = enrich_product(
            "SWOMCD.TXT",
            b"000 \nACUS11 KWNS 260208\nSWOMCD\nSPC MCD 260208\nMIZ000-WIZ000-260415-\n\nMESOSCALE DISCUSSION 1525\nNWS STORM PREDICTION CENTER NORMAN OK\n0908 PM CDT THU JUL 25 2013\n\nAREAS AFFECTED...PORTIONS OF NRN WI AND THE UPPER PENINSULA OF MI\n\nCONCERNING...SEVERE THUNDERSTORM WATCH 446...\n\nVALID 260208Z - 260415Z\n\nATTN...WFO...MQT...GRB...DLH...\n\nLAT...LON 44738786 45378992 45829078 46369061 46638962 46338801\n 45868698 44738786\n",
        );

        assert_eq!(enrichment.source, ProductEnrichmentSource::TextHeader);
        assert_eq!(enrichment.family, Some("mcd_bulletin"));
        assert!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_mcd)
                .is_some()
        );
        assert!(enrichment.body.is_none());
        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_mcd)
                .map(|bulletin| bulletin.discussion_number),
            Some(1525)
        );
    }

    #[test]
    fn rbg94e_products_route_to_structured_ero_enrichment() {
        let enrichment = enrich_product(
            "RBG94E.TXT",
            b"000 \nFOUS30 KWBC 132156\nRBG94E\nDay 1 Excessive Rainfall Threat Area\nValid 2156Z Tue Jul 13 2021 - 12Z Wed Jul 14 2021\n\nMARGINAL RISK OF RAINFALL EXCEEDING FFG TO THE RIGHT OF A LINE FROM\n20 SE GTF 20 E MBW 20 SW PSF.\n",
        );

        assert_eq!(enrichment.source, ProductEnrichmentSource::TextHeader);
        assert_eq!(enrichment.family, Some("ero_bulletin"));
        assert!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_ero)
                .is_some()
        );
        assert!(enrichment.body.is_none());
        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_ero)
                .map(|bulletin| bulletin.day),
            Some(1)
        );
    }

    #[test]
    fn ptsdy1_products_route_to_structured_spc_outlook_enrichment() {
        let enrichment = enrich_product(
            "PTSDY1.TXT",
            b"000 \nWUUS01 KWNS 071300\nPTSDY1\nVALID TIME 071300Z - 081200Z\n\n... CATEGORICAL ...\n\nMRGL 49061987 48451952 47761927 49061987\n",
        );

        assert_eq!(enrichment.source, ProductEnrichmentSource::TextHeader);
        assert_eq!(enrichment.family, Some("spc_outlook_bulletin"));
        assert!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_spc_outlook)
                .is_some()
        );
        assert!(enrichment.body.is_none());
        assert_eq!(
            enrichment
                .parsed
                .as_ref()
                .and_then(ProductArtifact::as_spc_outlook)
                .map(|bulletin| bulletin.days[0].day),
            Some(1)
        );
    }

    #[test]
    fn non_text_products_use_filename_classification() {
        let enrichment = enrich_product("RADUMSVY.GIF", b"ignored");

        assert_eq!(enrichment.source, ProductEnrichmentSource::FilenameNonText);
        assert_eq!(enrichment.family, Some("radar_graphic"));
        assert_eq!(enrichment.title, Some("Radar graphic"));
        assert!(enrichment.office.is_none());
        assert!(enrichment.header.is_none());
        assert!(enrichment.wmo_header.is_none());
        assert!(enrichment.parsed.is_none());
    }

    #[test]
    fn unknown_non_text_products_are_marked_unknown() {
        let enrichment = enrich_product("mystery.bin", b"ignored");

        assert_eq!(enrichment.source, ProductEnrichmentSource::Unknown);
        assert_eq!(enrichment.container, "raw");
        assert!(enrichment.family.is_none());
        assert!(enrichment.office.is_none());
        assert!(enrichment.wmo_header.is_none());
        assert!(enrichment.parsed.is_none());
        let json = serde_json::to_value(&enrichment).expect("enrichment serializes");
        assert!(json.get("flags").is_none());
    }

    #[test]
    fn zip_framed_txt_payload_is_treated_as_unknown_zip() {
        let enrichment = enrich_product("TAFALLUS.TXT", b"PK\x03\x04compressed bytes");

        assert_eq!(enrichment.source, ProductEnrichmentSource::Unknown);
        assert_eq!(enrichment.container, "zip");
        assert!(enrichment.family.is_none());
        assert!(enrichment.office.is_none());
        assert!(enrichment.header.is_none());
        assert!(enrichment.wmo_header.is_none());
        assert!(enrichment.parsed.is_none());
        assert!(enrichment.issues.is_empty());
    }
}
