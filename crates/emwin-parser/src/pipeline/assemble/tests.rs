use std::collections::BTreeMap;

use chrono::{TimeZone, Utc};

use crate::ParserError;
use crate::body::BodyInputFormat;
use crate::specialized::dcp::parse_dcp_bulletin;
use crate::specialized::fd::parse_fd_bulletin;
use crate::specialized::metar::{MetarBulletin, parse_metar_bulletin};
use crate::specialized::pirep::parse_pirep_bulletin;
use crate::specialized::sigmet::parse_sigmet_bulletin;
use crate::specialized::taf::parse_taf_bulletin;
use crate::{
    Cf6Amount, Cf6Bulletin, Cf6DayRow, CwaBulletin, CwaGeometry, CwaGeometryKind, DsmBulletin,
    DsmSummary, GeoPoint, HmlBulletin, HmlDatum, HmlDocument, HmlSeries, LsrBulletin, LsrReport,
    MosBulletin, MosForecastRow, MosSection, ProductArtifact, ProductEnrichmentSource,
    ProductParseIssue, SawAction, SawBulletin, SelBulletin, SpcWatchType, WwpBulletin,
};

use super::assemble_product_enrichment;
use crate::pipeline::candidate::{
    BodyContributionRequest, Cf6Candidate, ClassificationCandidate, CwaCandidate, DcpCandidate,
    DsmCandidate, FdCandidate, HmlCandidate, LsrCandidate, MetarCandidate, MosCandidate,
    PirepCandidate, SawCandidate, SelCandidate, SigmetCandidate, TafCandidate,
    TextGenericCandidate, UnsupportedWmoCandidate, WwpCandidate,
};

fn text_header(afos: &str) -> crate::TextProductHeader {
    crate::TextProductHeader {
        ttaaii: "FTUS42".to_string(),
        cccc: "KFFC".to_string(),
        ddhhmm: "022320".to_string(),
        bbb: None,
        afos: afos.to_string(),
    }
}

fn wmo_header(ttaaii: &str, cccc: &str) -> crate::WmoHeader {
    crate::WmoHeader {
        ttaaii: ttaaii.to_string(),
        cccc: cccc.to_string(),
        ddhhmm: "070200".to_string(),
        bbb: None,
    }
}

#[test]
fn assembles_text_generic_product_shape() {
    let candidate = ClassificationCandidate::TextGeneric(TextGenericCandidate {
        header: text_header("TAFPDK"),
        pil: Some("TAF".to_string()),
        title: Some("Terminal Aerodrome Forecast"),
        body_request: None,
        bbb_kind: None,
        reference_time: Some(Utc::now()),
    });

    let enrichment = assemble_product_enrichment(candidate, "TAFPDKGA.TXT", b"ignored");

    assert_eq!(
        enrichment.source,
        crate::ProductEnrichmentSource::TextHeader
    );
    assert_eq!(enrichment.pil.as_deref(), Some("TAF"));
    assert_eq!(enrichment.family, Some("nws_text_product"));
}

#[test]
fn assembles_fd_candidate_shape() {
    let reference_time = Utc
        .with_ymd_and_hms(2025, 3, 7, 0, 0, 0)
        .single()
        .expect("valid reference time");
    let bulletin = parse_fd_bulletin(
        "DATA BASED ON 070000Z\nVALID 071200Z\nFT 3000 6000\nBOS 9900 2812\n",
        Some("FD1US1"),
        reference_time,
    )
    .expect("fd bulletin should parse");
    let candidate = ClassificationCandidate::Fd(FdCandidate {
        source: crate::ProductEnrichmentSource::TextHeader,
        family: "fd_bulletin",
        title: "Winds and temperatures aloft",
        header: Some(text_header("FD1US1")),
        wmo_header: None,
        pil: Some("FD1".to_string()),
        bbb_kind: None,
        body_request: None,
        bulletin,
    });

    let enrichment = assemble_product_enrichment(candidate, "FD1US1.TXT", b"ignored");

    assert_eq!(enrichment.family, Some("fd_bulletin"));
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_fd)
            .is_some()
    );
}

#[test]
fn assembles_pirep_candidate_shape() {
    let bulletin = parse_pirep_bulletin("DEN UA /OV 35 SW /TM 1925 /FL050 /TP E145=\n")
        .expect("pirep bulletin should parse");
    let candidate = ClassificationCandidate::Pirep(PirepCandidate {
        source: ProductEnrichmentSource::TextHeader,
        header: Some(text_header("PIRBOU")),
        wmo_header: None,
        pil: Some("PIR".to_string()),
        bbb_kind: None,
        body_request: None,
        bulletin,
    });

    let enrichment = assemble_product_enrichment(candidate, "PIRBOU.TXT", b"ignored");

    assert_eq!(
        enrichment.source,
        crate::ProductEnrichmentSource::TextHeader
    );
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_pirep)
            .is_some()
    );
}

#[test]
fn assembles_sigmet_candidate_shape() {
    let bulletin = parse_sigmet_bulletin(
            "CONVECTIVE SIGMET 12C\nVALID UNTIL 2355Z\nIA MO\nFROM 20S DSM-30NW IRK\nAREA EMBD TS MOV FROM 24020KT.\n",
        )
        .expect("sigmet bulletin should parse");
    let candidate = ClassificationCandidate::Sigmet(SigmetCandidate {
        source: crate::ProductEnrichmentSource::TextHeader,
        header: Some(text_header("SIGABC")),
        wmo_header: None,
        pil: Some("SIG".to_string()),
        bbb_kind: None,
        body_request: None,
        bulletin,
        issues: Vec::new(),
    });

    let enrichment = assemble_product_enrichment(candidate, "SIGABC.TXT", b"ignored");

    assert_eq!(enrichment.family, Some("sigmet_bulletin"));
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_sigmet)
            .is_some()
    );
}

#[test]
fn assembles_lsr_candidate_shape() {
    let issues = vec![ProductParseIssue::new(
        "lsr_parse",
        "invalid_lsr_report",
        "could not parse LSR report block",
        Some("bad chunk".to_string()),
    )];
    let bulletin = LsrBulletin {
        reports: vec![LsrReport {
            valid: "2026-03-10T01:50:00+00:00".to_string(),
            event_text: "HAIL".to_string(),
            city: "BROOKSVILLE".to_string(),
            county: Some("WINSTON".to_string()),
            state: Some("AL".to_string()),
            latitude: 34.40,
            longitude: -87.70,
            source: Some("PUBLIC".to_string()),
            remark: Some("QUARTER SIZE HAIL".to_string()),
            magnitude_value: Some(1.0),
            magnitude_units: Some("IN".to_string()),
            magnitude_qualifier: None,
        }],
        is_summary: false,
    };
    let candidate = ClassificationCandidate::Lsr(LsrCandidate {
        header: text_header("LSRBMX"),
        pil: Some("LSR".to_string()),
        bbb_kind: None,
        body_request: None,
        bulletin,
        issues,
    });

    let enrichment = assemble_product_enrichment(candidate, "LSRBMX.TXT", b"ignored");

    assert_eq!(
        enrichment.source,
        crate::ProductEnrichmentSource::TextHeader
    );
    assert_eq!(enrichment.family, Some("lsr_bulletin"));
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_lsr)
            .is_some()
    );
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_cwa)
            .is_none()
    );
    assert!(enrichment.body.is_none());
    assert_eq!(enrichment.issues.len(), 1);
}

#[test]
fn assembles_cwa_candidate_shape() {
    let bulletin = CwaBulletin {
        center: "ZLC".to_string(),
        number: 202,
        issue_time: "2026-03-10T02:29:00+00:00".to_string(),
        expire_time: "2026-03-10T04:30:00+00:00".to_string(),
        is_corrected: false,
        is_cancelled: false,
        narrative: Some("AREA TS.".to_string()),
        geometry: Some(CwaGeometry {
            kind: CwaGeometryKind::Polygon,
            points: vec![
                GeoPoint {
                    lat: 40.7884,
                    lon: -111.9778,
                },
                GeoPoint {
                    lat: 44.7692,
                    lon: -106.9803,
                },
            ],
        }),
    };
    let candidate = ClassificationCandidate::Cwa(CwaCandidate {
        header: None,
        wmo_header: Some(wmo_header("FAUS22", "KZLC")),
        pil: Some("CWA".to_string()),
        bbb_kind: None,
        body_request: None,
        bulletin,
        issues: Vec::new(),
    });

    let enrichment = assemble_product_enrichment(candidate, "CWAZLC.TXT", b"ignored");

    assert_eq!(
        enrichment.source,
        crate::ProductEnrichmentSource::WmoBulletin
    );
    assert_eq!(enrichment.family, Some("cwa_bulletin"));
    assert!(enrichment.header.is_none());
    assert!(enrichment.wmo_header.is_some());
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_cwa)
            .is_some()
    );
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_wwp)
            .is_none()
    );
    assert!(enrichment.body.is_none());
}

#[test]
fn assembles_wwp_candidate_shape() {
    let bulletin = WwpBulletin {
        watch_type: SpcWatchType::Tornado,
        watch_number: 31,
        prob_tornadoes_2_or_more: 20,
        prob_tornadoes_1_or_more_strong: 10,
        prob_severe_wind_10_or_more: 70,
        prob_wind_1_or_more_65kt: 40,
        prob_severe_hail_10_or_more: 60,
        prob_hail_1_or_more_2inch: 30,
        prob_combined_hail_wind_6_or_more: 95,
        max_hail_inches: 2.0,
        max_wind_gust_knots: 70,
        max_tops_feet: 50_000,
        storm_motion_degrees: 240,
        storm_motion_knots: 35,
        is_pds: false,
    };
    let candidate = ClassificationCandidate::Wwp(WwpCandidate {
        header: text_header("WWP1"),
        pil: Some("WWP".to_string()),
        bbb_kind: None,
        body_request: None,
        bulletin,
        issues: Vec::new(),
    });

    let enrichment = assemble_product_enrichment(candidate, "WWP1.TXT", b"ignored");

    assert_eq!(
        enrichment.source,
        crate::ProductEnrichmentSource::TextHeader
    );
    assert_eq!(enrichment.family, Some("wwp_bulletin"));
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_wwp)
            .is_some()
    );
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_cf6)
            .is_none()
    );
    assert!(enrichment.body.is_none());
}

#[test]
fn assembles_saw_candidate_with_body_shape() {
    let candidate = ClassificationCandidate::Saw(SawCandidate {
        header: text_header("SAW2"),
        pil: Some("SAW".to_string()),
        bbb_kind: None,
        body_request: Some(BodyContributionRequest {
            text: "MAZ000-RIZ000-CWZ000-\nLAT...LON 41087082 39507704 41247704 42827082\n"
                .to_string(),
            plan: crate::body::body_extraction_plan(&[
                crate::BodyExtractorId::Ugc,
                crate::BodyExtractorId::LatLon,
            ]),
            reference_time: Some(Utc::now()),
            input_format: BodyInputFormat::PlainText,
        }),
        bulletin: SawBulletin {
            saw_number: 2,
            watch_number: 542,
            watch_type: SpcWatchType::SevereThunderstorm,
            action: SawAction::Issue,
            is_test: false,
            replaces_watch_number: None,
            valid_from: Some("2025-07-25T17:45:00+00:00".to_string()),
            valid_to: Some("2025-07-26T01:00:00+00:00".to_string()),
            polygon: Some(vec![GeoPoint {
                lat: 41.08,
                lon: -70.82,
            }]),
        },
        issues: Vec::new(),
    });

    let enrichment = assemble_product_enrichment(candidate, "SAW2.TXT", b"ignored");

    assert_eq!(
        enrichment.source,
        crate::ProductEnrichmentSource::TextHeader
    );
    assert_eq!(enrichment.family, Some("saw_bulletin"));
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_saw)
            .is_some()
    );
    assert!(enrichment.body.is_some());
}

#[test]
fn assembles_sel_candidate_with_body_shape() {
    let candidate = ClassificationCandidate::Sel(SelCandidate {
        header: text_header("SEL2"),
        pil: Some("SEL".to_string()),
        bbb_kind: None,
        body_request: Some(BodyContributionRequest {
            text: "IAC001-022320-\n".to_string(),
            plan: crate::body::body_extraction_plan(&[crate::BodyExtractorId::Ugc]),
            reference_time: Some(Utc::now()),
            input_format: BodyInputFormat::PlainText,
        }),
        bulletin: SelBulletin {
            watch_number: 542,
            watch_type: SpcWatchType::SevereThunderstorm,
            is_test: false,
        },
        issues: Vec::new(),
    });

    let enrichment = assemble_product_enrichment(candidate, "SEL2.TXT", b"ignored");

    assert_eq!(
        enrichment.source,
        crate::ProductEnrichmentSource::TextHeader
    );
    assert_eq!(enrichment.family, Some("sel_bulletin"));
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_sel)
            .is_some()
    );
    assert!(enrichment.body.is_some());
}

#[test]
fn assembles_cf6_candidate_shape() {
    let bulletin = Cf6Bulletin {
        station: "TEST STATION".to_string(),
        month: 3,
        year: 2026,
        rows: vec![Cf6DayRow {
            day: 1,
            max_temp_f: Some(70),
            min_temp_f: Some(50),
            avg_temp_f: Some(60),
            departure_f: Some(0),
            heating_degree_days: Some(5),
            cooling_degree_days: Some(0),
            precip_inches: Some(Cf6Amount::Trace),
            snow_inches: Some(Cf6Amount::Measured { inches: 0.0 }),
            snow_depth_inches: Some(Cf6Amount::Measured { inches: 0.0 }),
            avg_wind_mph: Some(8.5),
            max_wind_mph: Some(20),
            avg_wind_dir_degrees: Some(180),
            minutes_sunshine: Some(600),
            possible_sunshine_minutes: Some(720),
            sky_cover: Some("CLR".to_string()),
            weather_codes: Some("RA".to_string()),
            gust_mph: Some(30),
            gust_dir_degrees: Some(190),
        }],
    };
    let candidate = ClassificationCandidate::Cf6(Cf6Candidate {
        header: text_header("CF6GSN"),
        pil: Some("CF6".to_string()),
        bbb_kind: None,
        body_request: None,
        bulletin,
        issues: Vec::new(),
    });

    let enrichment = assemble_product_enrichment(candidate, "CF6GSN.TXT", b"ignored");

    assert_eq!(
        enrichment.source,
        crate::ProductEnrichmentSource::TextHeader
    );
    assert_eq!(enrichment.family, Some("cf6_bulletin"));
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_cf6)
            .is_some()
    );
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_dsm)
            .is_none()
    );
    assert!(enrichment.body.is_none());
    assert!(enrichment.issues.is_empty());
}

#[test]
fn assembles_dsm_candidate_shape() {
    let bulletin = DsmBulletin {
        summaries: vec![DsmSummary {
            station: "KCQC".to_string(),
            date: "2026-03-10".to_string(),
            max_temp_f: Some(63),
            max_temp_time: Some("2026-03-10T15:53:00+00:00".to_string()),
            min_temp_f: Some(40),
            min_temp_time: Some("2026-03-10T06:27:00+00:00".to_string()),
            coop_max_temp_f: Some(63),
            coop_min_temp_f: Some(40),
            min_sea_level_pressure_mb_tenths: Some(9671),
            min_slp_time: Some("2026-03-10T06:08:00+00:00".to_string()),
            precip_day_inches: Some(0.0),
            hourly_precip_inches: vec![Some(0.0); 24],
            avg_wind_mph: Some(28.0),
            max_wind_mph: Some(28.0),
            max_wind_time: Some("2026-03-10T20:59:00+00:00".to_string()),
            max_wind_dir_degrees: Some(280),
            max_gust_mph: Some(43.0),
            max_gust_time: Some("2026-03-10T15:31:00+00:00".to_string()),
            max_gust_dir_degrees: Some(290),
        }],
    };
    let candidate = ClassificationCandidate::Dsm(DsmCandidate {
        source: ProductEnrichmentSource::TextHeader,
        header: Some(text_header("DSMCQC")),
        wmo_header: None,
        pil: Some("DSM".to_string()),
        bbb_kind: None,
        body_request: None,
        bulletin,
        issues: Vec::new(),
    });

    let enrichment = assemble_product_enrichment(candidate, "DSMCQC.TXT", b"ignored");

    assert_eq!(
        enrichment.source,
        crate::ProductEnrichmentSource::TextHeader
    );
    assert_eq!(enrichment.family, Some("dsm_bulletin"));
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_dsm)
            .is_some()
    );
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_hml)
            .is_none()
    );
    assert!(enrichment.body.is_none());
}

#[test]
fn assembles_hml_candidate_shape() {
    let bulletin = HmlBulletin {
        documents: vec![HmlDocument {
            station_id: "AAMC1".to_string(),
            station_name: Some("ARROYO SECO".to_string()),
            originator: Some("MTR".to_string()),
            generation_time: Some("2026-03-10T00:02:00Z".to_string()),
            observed: Some(HmlSeries {
                issued: Some("2026-03-10T00:00:00Z".to_string()),
                primary_name: Some("Stage".to_string()),
                primary_units: Some("FT".to_string()),
                secondary_name: None,
                secondary_units: None,
                rows: vec![HmlDatum {
                    valid: "2026-03-10T00:00:00Z".to_string(),
                    primary: Some(2.5),
                    secondary: None,
                }],
            }),
            forecast: None,
        }],
    };
    let candidate = ClassificationCandidate::Hml(HmlCandidate {
        header: text_header("HMLMTR"),
        pil: Some("HML".to_string()),
        bbb_kind: None,
        body_request: None,
        bulletin,
        issues: Vec::new(),
    });

    let enrichment = assemble_product_enrichment(candidate, "HMLMTR.TXT", b"ignored");

    assert_eq!(
        enrichment.source,
        crate::ProductEnrichmentSource::TextHeader
    );
    assert_eq!(enrichment.family, Some("hml_bulletin"));
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_hml)
            .is_some()
    );
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_mos)
            .is_none()
    );
    assert!(enrichment.body.is_none());
}

#[test]
fn assembles_mos_candidate_shape() {
    let mut values = BTreeMap::new();
    values.insert("TMP".to_string(), "20".to_string());
    values.insert("WSP".to_string(), "05".to_string());
    let bulletin = MosBulletin {
        sections: vec![MosSection {
            station: "KBCK".to_string(),
            model: "NAM".to_string(),
            runtime: "2026-03-10T00:00:00Z".to_string(),
            forecasts: vec![MosForecastRow {
                valid: "2026-03-10T00:00:00Z".to_string(),
                values,
            }],
        }],
    };
    let candidate = ClassificationCandidate::Mos(MosCandidate {
        header: text_header("METNC1"),
        pil: Some("MET".to_string()),
        bbb_kind: None,
        body_request: None,
        bulletin,
        issues: Vec::new(),
    });

    let enrichment = assemble_product_enrichment(candidate, "METNC1.TXT", b"ignored");

    assert_eq!(
        enrichment.source,
        crate::ProductEnrichmentSource::TextHeader
    );
    assert_eq!(enrichment.family, Some("mos_bulletin"));
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_mos)
            .is_some()
    );
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_lsr)
            .is_none()
    );
    assert!(enrichment.body.is_none());
}

#[test]
fn assembles_metar_candidate_shape() {
    let (bulletin, issues) =
        parse_metar_bulletin("METAR BGKK 070220Z AUTO VRB02KT 9999NDV OVC043/// M03/M08 Q0967=\n")
            .expect("metar bulletin should parse");
    let candidate = ClassificationCandidate::Metar(MetarCandidate {
        source: ProductEnrichmentSource::WmoBulletin,
        header: None,
        wmo_header: Some(wmo_header("SAGL31", "BGGH")),
        pil: None,
        bbb_kind: None,
        body_request: None,
        bulletin,
        issues,
    });

    let enrichment = assemble_product_enrichment(candidate, "SAGL31.TXT", b"ignored");

    assert_eq!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_metar)
            .map(MetarBulletin::report_count),
        Some(1)
    );
}

#[test]
fn assembles_taf_candidate_shape() {
    let bulletin = parse_taf_bulletin("TAF AMD\nWBCF 070244Z 0703/0803 18012KT P6SM SCT050\n")
        .expect("taf bulletin should parse");
    let candidate = ClassificationCandidate::Taf(TafCandidate {
        source: ProductEnrichmentSource::WmoBulletin,
        header: None,
        wmo_header: Some(wmo_header("FTXX01", "KWBC")),
        pil: None,
        bbb_kind: None,
        body_request: None,
        bulletin,
    });

    let enrichment = assemble_product_enrichment(candidate, "TAFWBCFJ.TXT", b"ignored");

    assert_eq!(enrichment.family, Some("taf_bulletin"));
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_taf)
            .is_some()
    );
}

#[test]
fn assembles_dcp_candidate_shape() {
    let header = wmo_header("SXMS50", "KWAL");
    let bulletin = parse_dcp_bulletin(
        "MISDCPSV.TXT",
        &header,
        "83786162 066025814\n16.23\n003\n137\n071\n088\n12.9\n137\n007\n00000\n 42-0NN  45E\n",
    )
    .expect("dcp bulletin should parse");
    let candidate = ClassificationCandidate::Dcp(DcpCandidate { header, bulletin });

    let enrichment = assemble_product_enrichment(candidate, "MISDCPSV.TXT", b"ignored");

    assert_eq!(enrichment.family, Some("dcp_telemetry_bulletin"));
    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_dcp)
            .is_some()
    );
}

#[test]
fn assembles_unsupported_wmo_candidate_shape() {
    let candidate = ClassificationCandidate::UnsupportedWmo(UnsupportedWmoCandidate {
        family: "airmet_bulletin",
        title: Some("AIRMET bulletin"),
        header: wmo_header("WAAB31", "LATI"),
        code: "unsupported_airmet_bulletin",
        message: "recognized valid WMO AIRMET bulletin, but textual AIRMET parsing is not implemented",
        line: Some("LAAA AIRMET 1 VALID 090100/090500 LATI-".to_string()),
    });

    let enrichment = assemble_product_enrichment(candidate, "WAAB31.TXT", b"ignored");

    assert_eq!(
        enrichment.source,
        crate::ProductEnrichmentSource::WmoBulletin
    );
    assert_eq!(enrichment.family, Some("airmet_bulletin"));
    assert_eq!(enrichment.title, Some("AIRMET bulletin"));
    assert_eq!(enrichment.issues[0].code, "unsupported_airmet_bulletin");
}

#[test]
fn assembles_explicit_unsupported_wmo_family_shape() {
    let candidate = ClassificationCandidate::UnsupportedWmo(UnsupportedWmoCandidate {
        family: "canadian_tornado_warning_bulletin",
        title: Some("Canadian tornado warning bulletin"),
        header: wmo_header("WFCN11", "CWTO"),
        code: "unsupported_canadian_tornado_warning_bulletin",
        message: "recognized valid WMO Canadian tornado warning bulletin, but parsing is not implemented",
        line: Some("TORNADO WARNING FOR SOUTHERN ONTARIO.".to_string()),
    });

    let enrichment = assemble_product_enrichment(candidate, "TORW11CN.TXT", b"ignored");

    assert_eq!(enrichment.family, Some("canadian_tornado_warning_bulletin"));
    assert_eq!(enrichment.title, Some("Canadian tornado warning bulletin"));
    assert_eq!(
        enrichment.issues[0].code,
        "unsupported_canadian_tornado_warning_bulletin"
    );
}

#[test]
fn assembles_text_parse_failure_issue_shape() {
    let enrichment = assemble_product_enrichment(
        ClassificationCandidate::TextParseFailure(ParserError::InvalidWmoHeader {
            line: "INVALID HEADER".to_string(),
        }),
        "TAFPDKGA.TXT",
        b"ignored",
    );

    assert_eq!(
        enrichment.source,
        crate::ProductEnrichmentSource::TextHeader
    );
    assert_eq!(enrichment.issues[0].code, "invalid_wmo_header");
}

#[test]
fn assembles_unknown_non_text_shape() {
    let enrichment =
        assemble_product_enrichment(ClassificationCandidate::Unknown, "mystery.bin", b"ignored");

    assert_eq!(enrichment.source, crate::ProductEnrichmentSource::Unknown);
    assert_eq!(enrichment.container, "raw");
    assert!(enrichment.family.is_none());
}

#[test]
fn text_generic_candidate_assembles_body_from_plan() {
    let candidate = ClassificationCandidate::TextGeneric(TextGenericCandidate {
        header: text_header("TAFPDK"),
        pil: Some("TAF".to_string()),
        title: Some("Terminal Aerodrome Forecast"),
        body_request: Some(BodyContributionRequest {
            text: "/O.NEW.KDMX.TO.W.0001.250301T1200Z-250301T1300Z/".to_string(),
            plan: crate::data::text_product_catalog_entry("SVR")
                .and_then(crate::data::body_extraction_plan_for_entry)
                .expect("SVR should have body extraction plan"),
            reference_time: Some(Utc::now()),
            input_format: BodyInputFormat::PlainText,
        }),
        bbb_kind: None,
        reference_time: Some(Utc::now()),
    });

    let enrichment = assemble_product_enrichment(candidate, "TAFPDKGA.TXT", b"ignored");

    assert!(enrichment.body.is_some());
    assert!(
        enrichment
            .body
            .as_ref()
            .and_then(|body| body.as_vtec_event())
            .is_some()
    );
}

#[test]
fn specialized_candidates_without_body_request_remain_bodyless() {
    let bulletin = parse_pirep_bulletin("DEN UA /OV 35 SW /TM 1925 /FL050 /TP E145=\n")
        .expect("pirep bulletin should parse");
    let candidate = ClassificationCandidate::Pirep(PirepCandidate {
        source: ProductEnrichmentSource::TextHeader,
        header: Some(text_header("PIRBOU")),
        wmo_header: None,
        pil: Some("PIR".to_string()),
        bbb_kind: None,
        body_request: None,
        bulletin,
    });

    let enrichment = assemble_product_enrichment(candidate, "PIRBOU.TXT", b"ignored");

    assert!(enrichment.body.is_none());
    assert!(enrichment.issues.is_empty());
}

#[test]
fn body_request_issues_are_appended_to_text_generic_output() {
    let candidate = ClassificationCandidate::TextGeneric(TextGenericCandidate {
        header: text_header("ZZZBOX"),
        pil: None,
        title: None,
        body_request: Some(BodyContributionRequest {
            text: "plain text".to_string(),
            plan: crate::body::body_extraction_plan(&[crate::body::BodyExtractorId::TimeMotLoc]),
            reference_time: None,
            input_format: BodyInputFormat::PlainText,
        }),
        bbb_kind: None,
        reference_time: None,
    });

    let enrichment = assemble_product_enrichment(candidate, "ZZZBOX.TXT", b"ignored");

    assert_eq!(enrichment.issues.len(), 1);
    assert_eq!(enrichment.issues[0].code, "missing_reference_time");
}

#[test]
fn specialized_candidate_with_body_request_assembles_both_artifact_and_body() {
    let bulletin = parse_sigmet_bulletin(
            "CONVECTIVE SIGMET 12C\nVALID UNTIL 2355Z\nIA MO\nFROM 20S DSM-30NW IRK\nAREA EMBD TS MOV FROM 24020KT.\n",
        )
        .expect("sigmet bulletin should parse");
    let candidate = ClassificationCandidate::Sigmet(SigmetCandidate {
            source: crate::ProductEnrichmentSource::TextHeader,
            header: Some(text_header("SIGABC")),
            wmo_header: None,
            pil: Some("SIG".to_string()),
            bbb_kind: None,
            body_request: Some(BodyContributionRequest {
                text: "IAC001-011300-\n/O.NEW.KDMX.TO.W.0001.250301T1200Z-250301T1300Z/\nLAT...LON 4143 9613 4145 9610 4140 9608 4138 9612\n".to_string(),
                plan: crate::body::body_extraction_plan(&[
                    crate::body::BodyExtractorId::VtecEvents,
                ]),
                reference_time: Some(Utc::now()),
                input_format: BodyInputFormat::PlainText,
            }),
            bulletin,
            issues: Vec::new(),
        });

    let enrichment = assemble_product_enrichment(candidate, "SIGABC.TXT", b"ignored");

    assert!(
        enrichment
            .parsed
            .as_ref()
            .and_then(ProductArtifact::as_sigmet)
            .is_some()
    );
    assert!(enrichment.body.is_some());
    assert!(
        enrichment
            .body
            .as_ref()
            .and_then(|body| body.as_vtec_event())
            .is_some()
    );
}
