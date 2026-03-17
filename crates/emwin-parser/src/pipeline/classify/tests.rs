use chrono::{TimeZone, Utc};

use super::classify;
use super::common::build_body_request;
use super::context::TextClassificationContext;
use super::text::{
    classify_text_cf6, classify_text_cwa, classify_text_dsm, classify_text_fd, classify_text_hml,
    classify_text_lsr, classify_text_mos, classify_text_pirep, classify_text_saw,
    classify_text_sel, classify_text_wwp,
};
use crate::body::{BodyExtractorId, body_extraction_plan};
use crate::data::resolved_text_product_policy;
use crate::header::BbbKind;
use crate::pipeline::candidate::{ClassificationCandidate, FdCandidate};
use crate::pipeline::{NormalizedInput, ParsedEnvelope};
use crate::{ProductEnrichmentSource, TextProductHeader};

fn with_specialized_context<T>(
    pil: &'static str,
    afos: &'static str,
    body_text: &'static str,
    body_plan: Option<crate::body::BodyExtractionPlan>,
    f: impl FnOnce(&TextClassificationContext<'_>) -> T,
) -> T {
    let header = TextProductHeader {
        ttaaii: "FTUS80".to_string(),
        cccc: "KWBC".to_string(),
        ddhhmm: "100000".to_string(),
        bbb: None,
        afos: afos.to_string(),
    };
    let policy = resolved_text_product_policy(afos).expect("expected catalog policy");
    let context = TextClassificationContext {
        filename: "sample.TXT",
        header: &header,
        body_text,
        policy: Some(policy),
        pil: Some(pil.to_string()),
        title: Some(policy.title),
        body_plan,
        bbb_kind: None,
        reference_time: Some(Utc::now()),
    };

    f(&context)
}

#[test]
fn afos_fd_strategy_returns_fd_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "FD1US1.TXT",
        b"000 \nFTUS80 KWBC 070000\nFD1US1\nDATA BASED ON 070000Z\nVALID 071200Z\nFT 3000 6000\nBOS 9900 2812\n",
    ));

    assert!(matches!(
        classify(&envelope),
        ClassificationCandidate::Fd(_)
    ));
}

#[test]
fn afos_pirep_strategy_returns_pirep_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "PIRXXX.TXT",
        b"000 \nUAUS01 KBOU 070000\nPIRBOU\nDEN UA /OV 35 SW /TM 1925 /FL050 /TP E145=\n",
    ));

    assert!(matches!(
        classify(&envelope),
        ClassificationCandidate::Pirep(_)
    ));
}

#[test]
fn afos_sigmet_strategy_returns_sigmet_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "SIGABC.TXT",
        b"000 \nWSUS31 KKCI 070000\nSIGABC\nCONVECTIVE SIGMET 12C\nVALID UNTIL 2355Z\nIA MO\nFROM 20S DSM-30NW IRK\nAREA EMBD TS MOV FROM 24020KT.\n",
    ));

    assert!(matches!(
        classify(&envelope),
        ClassificationCandidate::Sigmet(_)
    ));
}

#[test]
fn local_lsr_sample_returns_lsr_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "LSRBMX.TXT",
        b"000 \nNWUS54 KBMX 100015\nLSRBMX\n..TIME...   ...EVENT...      ...CITY LOCATION...     ...LAT.LON...\n..DATE...   ....MAG....      ..COUNTY LOCATION..ST.. ...SOURCE....\n0150 AM     HAIL             BROOKSVILLE             34.40N 87.70W\n03/10/2026  1.00 IN          WINSTON             AL  PUBLIC\n&&\n",
    ));

    let ClassificationCandidate::Lsr(candidate) = classify(&envelope) else {
        panic!("expected lsr candidate");
    };

    assert!(candidate.header.afos.starts_with("LSR"));
    assert!(candidate.body_request.is_none());
    assert_eq!(candidate.bulletin.reports.len(), 1);
}

#[test]
fn local_cwa_active_sample_returns_wmo_only_cwa_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "CWAZLC.TXT",
        b"000 \nFAUS22 KZLC 100229\nZLC CWA 202 100229\nZLC CWA 202 VALID UNTIL 100430\nFROM SLC-SHR-DDY AREA TS.\n",
    ));

    let ClassificationCandidate::Cwa(candidate) = classify(&envelope) else {
        panic!("expected cwa candidate");
    };

    assert!(candidate.header.is_none());
    assert!(candidate.wmo_header.is_some());
    assert!(candidate.body_request.is_none());
    assert!(!candidate.bulletin.is_cancelled);
}

#[test]
fn local_cwa_cancel_sample_returns_wmo_only_cwa_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "CWAZFW.TXT",
        b"000 \nFAUS24 KZFW 100038\nZFW CWA 101 100038\nZFW CWA 101 VALID UNTIL 100200\nCANCEL CWA 101. ERROR CORRECTED.\n",
    ));

    let ClassificationCandidate::Cwa(candidate) = classify(&envelope) else {
        panic!("expected cwa candidate");
    };

    assert!(candidate.header.is_none());
    assert!(candidate.wmo_header.is_some());
    assert!(candidate.bulletin.is_cancelled);
}

#[test]
fn local_wwp_sample_returns_wwp_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "WWP1.TXT",
        b"000 \nWWUS40 KWNS 102008\nWWP1\nTORNADO WATCH PROBABILITIES FOR WT 0031\nPROBABILITY TABLE:\nPROB OF 2 OR MORE TORNADOES : 20%\nPROB OF 1 OR MORE STRONG /EF2-EF5/ TORNADOES : 10%\nPROB OF 10 OR MORE SEVERE WIND EVENTS : 70%\nPROB OF 1 OR MORE WIND EVENTS >= 65 KNOTS : 40%\nPROB OF 10 OR MORE SEVERE HAIL EVENTS : 60%\nPROB OF 1 OR MORE HAIL EVENTS >= 2 INCHES : 30%\nPROB OF 6 OR MORE COMBINED SEVERE HAIL/WIND EVENTS : 95%\nATTRIBUTE TABLE:\nMAX HAIL /INCHES/ : 2.0\nMAX WIND GUSTS SURFACE /KNOTS/ : 70\nMAX TOPS /X 100 FEET/ : 500\nMEAN STORM MOTION VECTOR /DEGREES AND KNOTS/ : 24035\nPARTICULARLY DANGEROUS SITUATION : NO\n",
    ));

    let ClassificationCandidate::Wwp(candidate) = classify(&envelope) else {
        panic!("expected wwp candidate");
    };

    assert!(candidate.header.afos.starts_with("WWP"));
    assert!(candidate.body_request.is_none());
    assert_eq!(candidate.bulletin.watch_number, 31);
}

#[test]
fn local_saw_sample_returns_saw_candidate_with_body_request() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "SAW2.TXT",
        b"000 \nWWUS30 KWNS 251740\nSAW2\n\x1eSPC AWW 251740\nWW 542 SEVERE TSTM CT DE MA NJ NY PA RI CW 251745Z - 260100Z\nLAT...LON 41087082 39507704 41247704 42827082\n",
    ));

    let ClassificationCandidate::Saw(candidate) = classify(&envelope) else {
        panic!("expected saw candidate");
    };

    assert!(candidate.header.afos.starts_with("SAW"));
    assert!(candidate.body_request.is_some());
    assert_eq!(candidate.bulletin.watch_number, 542);
}

#[test]
fn local_sel_sample_returns_sel_candidate_with_body_request() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "SEL2.TXT",
        b"844 \nWWUS20 KWNS 251740\nSEL2  \n\x1eSPC WW 251740\nCTZ000-DEZ000-MAZ000-NJZ000-NYZ000-PAZ000-RIZ000-CWZ000-260100-\n\nURGENT - IMMEDIATE BROADCAST REQUESTED\nSevere Thunderstorm Watch Number 542\nNWS Storm Prediction Center Norman OK\n145 PM EDT Fri Jul 25 2025\n",
    ));

    let ClassificationCandidate::Sel(candidate) = classify(&envelope) else {
        panic!("expected sel candidate");
    };

    assert!(candidate.header.afos.starts_with("SEL"));
    assert!(candidate.body_request.is_some());
    assert_eq!(candidate.bulletin.watch_number, 542);
}

#[test]
fn local_cf6_sample_returns_cf6_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "CF6GSN.TXT",
        b"000 \nCXGM50 PGUM 100030\nCF6GSN\nPRELIMINARY LOCAL CLIMATOLOGICAL DATA\nSTATION: TEST STATION\nMONTH: MARCH\nYEAR: 2026\nDY MAX MIN AVG DEP HDD CDD PCP SNW SND AWD MWD DIR MIN PSBL SKY WX GST GDR\n 1 70 50 60 0 5 0 0.10 0.0 0 8.5 20 180 600 720 CLR RA 30 190\n",
    ));

    let ClassificationCandidate::Cf6(candidate) = classify(&envelope) else {
        panic!("expected cf6 candidate");
    };

    assert!(candidate.header.afos.starts_with("CF6"));
    assert!(candidate.body_request.is_none());
    assert_eq!(candidate.bulletin.rows.len(), 1);
}

#[test]
fn local_dsm_sample_returns_dsm_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "DSMCQC.TXT",
        b"000 \nCXUS45 KABQ 110415\nDSMCQC\nKCQC DS 2100 10/03 631553/ 400627// 63/ 40//9671608/T/00/00/00/T/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/-/-/-/-/28282059/29431531\n",
    ));

    let ClassificationCandidate::Dsm(candidate) = classify(&envelope) else {
        panic!("expected dsm candidate");
    };

    assert!(
        candidate
            .header
            .as_ref()
            .is_some_and(|header| header.afos.starts_with("DSM"))
    );
    assert!(candidate.body_request.is_none());
    assert_eq!(candidate.bulletin.summaries[0].station, "KCQC");
}

#[test]
fn local_hml_sample_returns_hml_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
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
    ));

    let ClassificationCandidate::Hml(candidate) = classify(&envelope) else {
        panic!("expected hml candidate");
    };

    assert!(candidate.header.afos.starts_with("HML"));
    assert!(candidate.body_request.is_none());
    assert!(!candidate.bulletin.documents.is_empty());
}

#[test]
fn local_met_sample_returns_mos_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "METBCK.TXT",
        b"000 \nFOUS46 KWNO 100000\nMETBCK\nKBCK NAM MET GUIDANCE 03/10/2026 0000 UTC\nHR 00 03 06\nTMP 20 21 22\nWND 05 06 07\n",
    ));

    let ClassificationCandidate::Mos(candidate) = classify(&envelope) else {
        panic!("expected mos candidate");
    };

    assert!(candidate.header.afos.starts_with("MET"));
    assert!(candidate.body_request.is_none());
    assert_eq!(candidate.bulletin.sections[0].station, "KBCK");
}

#[test]
fn local_ftp_sample_returns_mos_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "FTPACR.TXT",
        b"000 \nFOAK12 KWNO 100000\nFTPACR\n.B FTP 0310 DH06/DC03100600\nAHP 12/08/13/09\n",
    ));

    let ClassificationCandidate::Mos(candidate) = classify(&envelope) else {
        panic!("expected mos candidate");
    };

    assert!(candidate.header.afos.starts_with("FTP"));
    assert!(candidate.body_request.is_none());
    assert_eq!(candidate.bulletin.sections[0].station, "AHP");
}

#[test]
fn afos_generic_fallback_returns_text_generic_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "TAFPDKGA.TXT",
        b"000 \nFTUS42 KFFC 022320\nTAFPDK\nBody\n",
    ));

    assert!(matches!(
        classify(&envelope),
        ClassificationCandidate::TextGeneric(_)
    ));
}

#[test]
fn text_generic_candidate_uses_catalog_body_behavior() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "SVRDMX.TXT",
        b"000 \nWUUS53 KDMX 022320\nSVRDMX\n/O.NEW.KDMX.SV.W.0001.250301T1200Z-250301T1300Z/\n",
    ));

    let ClassificationCandidate::TextGeneric(candidate) = classify(&envelope) else {
        panic!("expected generic text candidate");
    };

    assert!(candidate.body_request.is_some());
}

#[test]
fn text_generic_candidate_omits_body_request_when_catalog_body_behavior_is_never() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "ZZZXXX.TXT",
        b"000 \nFXUS61 KBOX 022101\nZZZBOX\nBody\n",
    ));

    let ClassificationCandidate::TextGeneric(candidate) = classify(&envelope) else {
        panic!("expected generic text candidate");
    };

    assert!(candidate.body_request.is_none());
}

#[test]
fn afos_strategy_precedence_prefers_pirep_when_catalog_routing_matches() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "PIRXXX.TXT",
        b"000 \nUAUS01 KBOU 070000\nPIRBOU\nDEN UA /OV 35 SW /TM 1925 /FL050 /TP E145=\nCONVECTIVE SIGMET 12C\nVALID UNTIL 2355Z\nIA MO\nFROM 20S DSM-30NW IRK\nAREA EMBD TS MOV FROM 24020KT.\n",
    ));

    assert!(matches!(
        classify(&envelope),
        ClassificationCandidate::Pirep(_)
    ));
}

#[test]
fn fd_candidate_has_no_body_request_by_default() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "FD1US1.TXT",
        b"000 \nFTUS80 KWBC 070000\nFD1US1\nDATA BASED ON 070000Z\nVALID 071200Z\nFT 3000 6000\nBOS 9900 2812\n",
    ));

    let ClassificationCandidate::Fd(candidate) = classify(&envelope) else {
        panic!("expected fd candidate");
    };

    assert!(candidate.body_request.is_none());
}

#[test]
fn fd_candidate_body_request_follows_catalog_policy() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "FD1US1.TXT",
        b"000 \nFTUS80 KWBC 070000\nFD1US1\nDATA BASED ON 070000Z\nVALID 071200Z\nFT 3000 6000\nBOS 9900 2812\n",
    ));

    let ClassificationCandidate::Fd(candidate) = classify(&envelope) else {
        panic!("expected fd candidate");
    };

    assert!(candidate.body_request.is_none());
}

#[test]
fn pirep_candidate_body_request_follows_catalog_policy() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "PIRXXX.TXT",
        b"000 \nUAUS01 KBOU 070000\nPIRBOU\nDEN UA /OV 35 SW /TM 1925 /FL050 /TP E145=\n",
    ));

    let ClassificationCandidate::Pirep(candidate) = classify(&envelope) else {
        panic!("expected pirep candidate");
    };

    assert!(candidate.body_request.is_none());
}

#[test]
fn sigmet_candidate_body_request_follows_catalog_policy() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "SIGABC.TXT",
        b"000 \nWSUS31 KKCI 070000\nSIGABC\nCONVECTIVE SIGMET 12C\nVALID UNTIL 2355Z\nIA MO\nFROM 20S DSM-30NW IRK\nAREA EMBD TS MOV FROM 24020KT.\n",
    ));

    let ClassificationCandidate::Sigmet(candidate) = classify(&envelope) else {
        panic!("expected sigmet candidate");
    };

    assert!(candidate.body_request.is_none());
}

#[test]
fn lsr_candidate_body_request_follows_catalog_policy() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "LSRBMX.TXT",
        b"000 \nNWUS54 KBMX 100015\nLSRBMX\n..TIME...   ...EVENT...      ...CITY LOCATION...     ...LAT.LON...\n..DATE...   ....MAG....      ..COUNTY LOCATION..ST.. ...SOURCE....\n0150 AM     HAIL             BROOKSVILLE             34.40N 87.70W\n03/10/2026  1.00 IN          WINSTON             AL  PUBLIC\n&&\n",
    ));

    let ClassificationCandidate::Lsr(candidate) = classify(&envelope) else {
        panic!("expected lsr candidate");
    };

    assert!(candidate.body_request.is_none());
}

#[test]
fn cwa_candidate_body_request_follows_catalog_policy() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "CWAZLC.TXT",
        b"000 \nFAUS22 KZLC 100229\nCWAZLC\nZLC2 CWA 100230\nZLC CWA 202 VALID UNTIL 100630\nFROM 75W BIL-15NNE SHR-55SW DDY-45S OCS-35SSE SLC-75W BIL\nAREA MOD/ISO SEV MTN WAVE FL350-ABV FL450. ALTITUDE CHANGE OF +/-25KTS. RPRTD BY ACFT. VISIBLE ON SATELLITE. CWSU 100230Z. CO ID MT UT WY\n=\n",
    ));

    let ClassificationCandidate::Cwa(candidate) = classify(&envelope) else {
        panic!("expected cwa candidate");
    };

    assert!(candidate.body_request.is_none());
}

#[test]
fn wwp_candidate_body_request_follows_catalog_policy() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "WWP1.TXT",
        b"000 \nWWUS40 KWNS 102008\nWWP1\nTORNADO WATCH PROBABILITIES FOR WT 0031\nPROBABILITY TABLE:\nPROB OF 2 OR MORE TORNADOES : 20%\nPROB OF 1 OR MORE STRONG /EF2-EF5/ TORNADOES : 10%\nPROB OF 10 OR MORE SEVERE WIND EVENTS : 70%\nPROB OF 1 OR MORE WIND EVENTS >= 65 KNOTS : 40%\nPROB OF 10 OR MORE SEVERE HAIL EVENTS : 60%\nPROB OF 1 OR MORE HAIL EVENTS >= 2 INCHES : 30%\nPROB OF 6 OR MORE COMBINED SEVERE HAIL/WIND EVENTS : 95%\nATTRIBUTE TABLE:\nMAX HAIL /INCHES/ : 2.0\nMAX WIND GUSTS SURFACE /KNOTS/ : 70\nMAX TOPS /X 100 FEET/ : 500\nMEAN STORM MOTION VECTOR /DEGREES AND KNOTS/ : 24035\nPARTICULARLY DANGEROUS SITUATION : NO\n",
    ));

    let ClassificationCandidate::Wwp(candidate) = classify(&envelope) else {
        panic!("expected wwp candidate");
    };

    assert!(candidate.body_request.is_none());
}

#[test]
fn cf6_candidate_body_request_follows_catalog_policy() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "CF6GSN.TXT",
        b"000 \nCXGM50 PGUM 100030\nCF6GSN\nPRELIMINARY LOCAL CLIMATOLOGICAL DATA\nSTATION: TEST STATION\nMONTH: MARCH\nYEAR: 2026\nDY MAX MIN AVG DEP HDD CDD PCP SNW SND AWD MWD DIR MIN PSBL SKY WX GST GDR\n 1 70 50 60 0 5 0 0.10 0.0 0 8.5 20 180 600 720 CLR RA 30 190\n",
    ));

    let ClassificationCandidate::Cf6(candidate) = classify(&envelope) else {
        panic!("expected cf6 candidate");
    };

    assert!(candidate.body_request.is_none());
}

#[test]
fn dsm_candidate_body_request_follows_catalog_policy() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "DSMCQC.TXT",
        b"000 \nCXUS45 KABQ 110415\nDSMCQC\nKCQC DS 2100 10/03 631553/ 400627// 63/ 40//9671608/T/00/00/00/T/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/-/-/-/-/28282059/29431531\n",
    ));

    let ClassificationCandidate::Dsm(candidate) = classify(&envelope) else {
        panic!("expected dsm candidate");
    };

    assert!(candidate.body_request.is_none());
}

#[test]
fn hml_candidate_body_request_follows_catalog_policy() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
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
    ));

    let ClassificationCandidate::Hml(candidate) = classify(&envelope) else {
        panic!("expected hml candidate");
    };

    assert!(candidate.body_request.is_none());
}

#[test]
fn mos_candidate_body_request_follows_catalog_policy() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "METBCK.TXT",
        b"000 \nFOUS46 KWNO 100000\nMETBCK\nKBCK NAM MET GUIDANCE 03/10/2026 0000 UTC\nHR 00 03 06\nTMP 20 21 22\nWND 05 06 07\n",
    ));

    let ClassificationCandidate::Mos(candidate) = classify(&envelope) else {
        panic!("expected mos candidate");
    };

    assert!(candidate.body_request.is_none());
}

#[test]
fn specialized_strategy_requires_matching_catalog_routing() {
    let header = TextProductHeader {
        ttaaii: "FTUS80".to_string(),
        cccc: "KWBC".to_string(),
        ddhhmm: "070000".to_string(),
        bbb: None,
        afos: "FD1US1".to_string(),
    };
    let context = TextClassificationContext {
        filename: "FD1US1.TXT",
        header: &header,
        body_text: "DATA BASED ON 070000Z\nVALID 071200Z\nFT 3000 6000\nBOS 9900 2812\n",
        policy: resolved_text_product_policy("PIRBOU"),
        pil: Some("FD1".to_string()),
        title: Some("Winds and Temperatures Aloft"),
        body_plan: None,
        bbb_kind: None,
        reference_time: Some(Utc::now()),
    };

    assert!(classify_text_fd(&context).is_none());
}

#[test]
fn saw_strategy_requires_matching_catalog_routing() {
    with_specialized_context(
        "SEL",
        "SEL2",
        "URGENT - IMMEDIATE BROADCAST REQUESTED\nSevere Thunderstorm Watch Number 542\n",
        Some(body_extraction_plan(&[BodyExtractorId::Ugc])),
        |context| assert!(classify_text_saw(context).is_none()),
    );
}

#[test]
fn sel_strategy_requires_matching_catalog_routing() {
    with_specialized_context(
        "SAW",
        "SAW2",
        "SPC AWW 251740\nWW 542 SEVERE TSTM CT DE MA NJ NY PA RI CW 251745Z - 260100Z\nLAT...LON 41087082 39507704 41247704 42827082\n",
        Some(body_extraction_plan(&[
            BodyExtractorId::Ugc,
            BodyExtractorId::LatLon,
        ])),
        |context| assert!(classify_text_sel(context).is_none()),
    );
}

#[test]
fn pirep_strategy_requires_matching_catalog_routing() {
    with_specialized_context(
        "LSR",
        "LSRBMX",
        "DEN UA /OV 35 SW /TM 1925 /FL050 /TP E145=\n",
        None,
        |context| assert!(classify_text_pirep(context).is_none()),
    );
}

#[test]
fn lsr_strategy_requires_matching_catalog_routing() {
    with_specialized_context(
        "PIR",
        "PIRBOU",
        "..TIME...   ...EVENT...      ...CITY LOCATION...     ...LAT.LON...\n..DATE...   ....MAG....      ..COUNTY LOCATION..ST.. ...SOURCE....\n0150 AM     HAIL             BROOKSVILLE             34.40N 87.70W\n03/10/2026  1.00 IN          WINSTON             AL  PUBLIC\n&&\n",
        None,
        |context| assert!(classify_text_lsr(context).is_none()),
    );
}

#[test]
fn cf6_strategy_requires_matching_catalog_routing() {
    with_specialized_context(
        "DSM",
        "DSMCQC",
        "PRELIMINARY LOCAL CLIMATOLOGICAL DATA\nSTATION: TEST STATION\nMONTH: MARCH\nYEAR: 2026\n",
        None,
        |context| assert!(classify_text_cf6(context).is_none()),
    );
}

#[test]
fn dsm_strategy_requires_matching_catalog_routing() {
    with_specialized_context(
        "CF6",
        "CF6GSN",
        "KCQC DS 2100 10/03 631553/ 400627// 63/ 40//9671608/T/00/00/00/T/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/-/-/-/-/28282059/29431531\n",
        None,
        |context| assert!(classify_text_dsm(context).is_none()),
    );
}

#[test]
fn mos_strategy_requires_matching_catalog_routing() {
    with_specialized_context(
        "PIR",
        "PIRBOU",
        "KBCK NAM MET GUIDANCE 03/10/2026 0000 UTC\nHR 00 03 06\nTMP 20 21 22\n",
        None,
        |context| assert!(classify_text_mos(context).is_none()),
    );
}

#[test]
fn unrouted_pirep_like_afos_falls_back_to_generic() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "ZZZBOX.TXT",
        b"000 \nFXUS61 KBOX 022101\nZZZBOX\nDEN UA /OV 35 SW /TM 1925 /FL050 /TP E145=\n",
    ));

    assert!(matches!(
        classify(&envelope),
        ClassificationCandidate::TextGeneric(_)
    ));
}

#[test]
fn unrouted_lsr_like_afos_falls_back_to_generic() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "ZZZBOX.TXT",
        b"000 \nFXUS61 KBOX 022101\nZZZBOX\n..TIME...   ...EVENT...      ...CITY LOCATION...     ...LAT.LON...\n..DATE...   ....MAG....      ..COUNTY LOCATION..ST.. ...SOURCE....\n0150 AM     HAIL             BROOKSVILLE             34.40N 87.70W\n03/10/2026  1.00 IN          WINSTON             AL  PUBLIC\n&&\n",
    ));

    assert!(matches!(
        classify(&envelope),
        ClassificationCandidate::TextGeneric(_)
    ));
}

#[test]
fn unrouted_cf6_like_afos_falls_back_to_generic() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "ZZZBOX.TXT",
        b"000 \nFXUS61 KBOX 022101\nZZZBOX\nPRELIMINARY LOCAL CLIMATOLOGICAL DATA\nSTATION: TEST STATION\nMONTH: MARCH\nYEAR: 2026\n",
    ));

    assert!(matches!(
        classify(&envelope),
        ClassificationCandidate::TextGeneric(_)
    ));
}

#[test]
fn unrouted_dsm_like_afos_falls_back_to_generic() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "ZZZBOX.TXT",
        b"000 \nFXUS61 KBOX 022101\nZZZBOX\nKCQC DS 2100 10/03 631553/ 400627// 63/ 40//9671608/T/00/00/00/T/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/-/-/-/-/28282059/29431531\n",
    ));

    assert!(matches!(
        classify(&envelope),
        ClassificationCandidate::TextGeneric(_)
    ));
}

#[test]
fn unrouted_mos_like_afos_falls_back_to_generic() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "ZZZBOX.TXT",
        b"000 \nFXUS61 KBOX 022101\nZZZBOX\nKBCK NAM MET GUIDANCE 03/10/2026 0000 UTC\nHR 00 03 06\nTMP 20 21 22\nWND 05 06 07\n",
    ));

    assert!(matches!(
        classify(&envelope),
        ClassificationCandidate::TextGeneric(_)
    ));
}

#[test]
fn malformed_lsr_stays_in_family_with_issue() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "LSRBMX.TXT",
        b"000 \nNWUS54 KBMX 100015\nLSRBMX\nPreliminary Local Storm Report\nNo standard report block\n",
    ));

    let ClassificationCandidate::MalformedFamily(candidate) = classify(&envelope) else {
        panic!("expected malformed-family candidate");
    };
    assert_eq!(candidate.family, "lsr_bulletin");
    assert_eq!(candidate.issues[0].code, "invalid_lsr_bulletin");
}

#[test]
fn malformed_wwp_stays_in_family_with_issue() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "WWP1.TXT",
        b"000 \nWWUS40 KWNS 102012\nWWP1\nTORNADO WATCH PROBABILITIES FOR WT 0031\nPROBABILITY TABLE:\n",
    ));

    let ClassificationCandidate::MalformedFamily(candidate) = classify(&envelope) else {
        panic!("expected malformed-family candidate");
    };
    assert_eq!(candidate.family, "wwp_bulletin");
    assert_eq!(candidate.issues[0].code, "invalid_wwp_bulletin");
}

#[test]
fn malformed_cf6_stays_in_family_with_issue() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "CF6GSN.TXT",
        b"000 \nCXGM50 PGUM 100030\nCF6GSN\nPRELIMINARY LOCAL CLIMATOLOGICAL DATA\nMONTH: MARCH\nYEAR: 2026\nDY MAX MIN AVG DEP HDD CDD WTR\n",
    ));

    let ClassificationCandidate::MalformedFamily(candidate) = classify(&envelope) else {
        panic!("expected malformed-family candidate");
    };
    assert_eq!(candidate.family, "cf6_bulletin");
    assert_eq!(candidate.issues[0].code, "invalid_cf6_bulletin");
}

#[test]
fn malformed_hml_stays_in_family_with_issue() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "HMLMTR.TXT",
        b"000 \nSRUS56 KMTR 100002\nHMLMTR\n<?xml version=\"1.0\"?><site><observed><datum></site>\n",
    ));

    let ClassificationCandidate::MalformedFamily(candidate) = classify(&envelope) else {
        panic!("expected malformed-family candidate");
    };
    assert_eq!(candidate.family, "hml_bulletin");
    assert_eq!(candidate.issues[0].code, "invalid_hml_bulletin");
}

#[test]
fn malformed_standard_mos_stays_in_family_with_issue() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "METBCK.TXT",
        b"000 \nFOUS46 KWNO 100000\nMETNC1\nKBCK   NAM MOS GUIDANCE    3/10/2026  0000 UTC\nTMP 41 38 36\n",
    ));

    let ClassificationCandidate::MalformedFamily(candidate) = classify(&envelope) else {
        panic!("expected malformed-family candidate");
    };
    assert_eq!(candidate.family, "mos_bulletin");
    assert_eq!(candidate.issues[0].code, "invalid_mos_bulletin");
}

#[test]
fn malformed_ftp_stays_in_family_with_issue() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "FTPACR.TXT",
        b"000 \nFOAK12 KWNO 100000\nFTPACR\n.B NMC 0311\n.B1 bad header\n.END\n",
    ));

    let ClassificationCandidate::MalformedFamily(candidate) = classify(&envelope) else {
        panic!("expected malformed-family candidate");
    };
    assert_eq!(candidate.family, "mos_bulletin");
    assert_eq!(candidate.issues[0].code, "invalid_mos_bulletin");
}

#[test]
fn invalid_wmo_only_non_cwa_does_not_hit_cwa_strategy() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "FXUS61.TXT",
        b"FXUS61 KBOX 022101\nAREA FORECAST DISCUSSION\n",
    ));

    assert!(matches!(
        classify(&envelope),
        ClassificationCandidate::UnsupportedWmo(_)
    ));
}

#[test]
fn specialized_candidate_can_carry_body_request_when_metadata_enables_catalog_body_behavior() {
    let reference_time = Utc
        .with_ymd_and_hms(2025, 3, 7, 0, 0, 0)
        .single()
        .expect("valid reference time");
    let request = build_body_request(
        Some(body_extraction_plan(&[BodyExtractorId::WindHail])),
        "/O.NEW.KDMX.TO.W.0001.250301T1200Z-250301T1300Z/",
        Some(reference_time),
    );

    let candidate = ClassificationCandidate::Fd(FdCandidate {
        source: ProductEnrichmentSource::TextHeader,
        family: "fd_bulletin",
        title: "Winds and temperatures aloft",
        header: Some(TextProductHeader {
            ttaaii: "FTUS80".to_string(),
            cccc: "KWBC".to_string(),
            ddhhmm: "070000".to_string(),
            bbb: None,
            afos: "FD1US1".to_string(),
        }),
        wmo_header: None,
        pil: Some("FD1".to_string()),
        bbb_kind: Some(BbbKind::Amendment),
        body_request: request,
        bulletin: crate::specialized::fd::parse_fd_bulletin(
            "DATA BASED ON 070000Z\nVALID 071200Z\nFT 3000 6000\nBOS 9900 2812\n",
            Some("FD1US1"),
            reference_time,
        )
        .expect("fd bulletin should parse"),
    });

    let ClassificationCandidate::Fd(candidate) = candidate else {
        panic!("expected fd candidate");
    };
    assert!(candidate.body_request.is_some());
}

#[test]
fn lsr_classifier_carries_body_request_when_plan_is_enabled() {
    let candidate = with_specialized_context(
        "LSR",
        "LSRBMX",
        "..TIME...   ...EVENT...      ...CITY LOCATION...     ...LAT.LON...\n..DATE...   ....MAG....      ..COUNTY LOCATION..ST.. ...SOURCE....\n0150 AM     HAIL             BROOKSVILLE             34.40N 87.70W\n03/10/2026  1.00 IN          WINSTON             AL  PUBLIC\n&&\n",
        Some(body_extraction_plan(&[BodyExtractorId::WindHail])),
        classify_text_lsr,
    );

    let ClassificationCandidate::Lsr(candidate) = candidate.expect("expected lsr candidate") else {
        panic!("expected lsr candidate");
    };
    assert!(candidate.body_request.is_some());
}

#[test]
fn cwa_classifier_carries_body_request_when_plan_is_enabled() {
    let candidate = with_specialized_context(
        "CWA",
        "CWAZLC",
        "ZLC2 CWA 100230\nZLC CWA 202 VALID UNTIL 100630\nFROM 75W BIL-15NNE SHR-55SW DDY-45S OCS-35SSE SLC-75W BIL\nAREA MOD/ISO SEV MTN WAVE FL350-ABV FL450. ALTITUDE CHANGE OF +/-25KTS. RPRTD BY ACFT. VISIBLE ON SATELLITE. CWSU 100230Z. CO ID MT UT WY\n=\n",
        Some(body_extraction_plan(&[BodyExtractorId::WindHail])),
        classify_text_cwa,
    );

    let ClassificationCandidate::Cwa(candidate) = candidate.expect("expected cwa candidate") else {
        panic!("expected cwa candidate");
    };
    assert!(candidate.body_request.is_some());
}

#[test]
fn wwp_classifier_carries_body_request_when_plan_is_enabled() {
    let candidate = with_specialized_context(
        "WWP",
        "WWP1",
        "TORNADO WATCH PROBABILITIES FOR WT 0031\nPROBABILITY TABLE:\nPROB OF 2 OR MORE TORNADOES : 20%\nPROB OF 1 OR MORE STRONG /EF2-EF5/ TORNADOES : 10%\nPROB OF 10 OR MORE SEVERE WIND EVENTS : 70%\nPROB OF 1 OR MORE WIND EVENTS >= 65 KNOTS : 40%\nPROB OF 10 OR MORE SEVERE HAIL EVENTS : 60%\nPROB OF 1 OR MORE HAIL EVENTS >= 2 INCHES : 30%\nPROB OF 6 OR MORE COMBINED SEVERE HAIL/WIND EVENTS : 95%\nATTRIBUTE TABLE:\nMAX HAIL /INCHES/ : 2.0\nMAX WIND GUSTS SURFACE /KNOTS/ : 70\nMAX TOPS /X 100 FEET/ : 500\nMEAN STORM MOTION VECTOR /DEGREES AND KNOTS/ : 24035\nPARTICULARLY DANGEROUS SITUATION : NO\n",
        Some(body_extraction_plan(&[BodyExtractorId::WindHail])),
        classify_text_wwp,
    );

    let ClassificationCandidate::Wwp(candidate) = candidate.expect("expected wwp candidate") else {
        panic!("expected wwp candidate");
    };
    assert!(candidate.body_request.is_some());
}

#[test]
fn cf6_classifier_carries_body_request_when_plan_is_enabled() {
    let candidate = with_specialized_context(
        "CF6",
        "CF6GSN",
        "PRELIMINARY LOCAL CLIMATOLOGICAL DATA\nSTATION: TEST STATION\nMONTH: MARCH\nYEAR: 2026\nDY MAX MIN AVG DEP HDD CDD PCP SNW SND AWD MWD DIR MIN PSBL SKY WX GST GDR\n 1 70 50 60 0 5 0 0.10 0.0 0 8.5 20 180 600 720 CLR RA 30 190\n",
        Some(body_extraction_plan(&[BodyExtractorId::WindHail])),
        classify_text_cf6,
    );

    let ClassificationCandidate::Cf6(candidate) = candidate.expect("expected cf6 candidate") else {
        panic!("expected cf6 candidate");
    };
    assert!(candidate.body_request.is_some());
}

#[test]
fn dsm_classifier_carries_body_request_when_plan_is_enabled() {
    let candidate = with_specialized_context(
        "DSM",
        "DSMCQC",
        "KCQC DS 2100 10/03 631553/ 400627// 63/ 40//9671608/T/00/00/00/T/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/00/-/-/-/-/28282059/29431531\n",
        Some(body_extraction_plan(&[BodyExtractorId::WindHail])),
        classify_text_dsm,
    );

    let ClassificationCandidate::Dsm(candidate) = candidate.expect("expected dsm candidate") else {
        panic!("expected dsm candidate");
    };
    assert!(candidate.body_request.is_some());
}

#[test]
fn hml_classifier_carries_body_request_when_plan_is_enabled() {
    let candidate = with_specialized_context(
        "HML",
        "HMLMTR",
        "<?xml version=\"1.0\"?>\n<site id=\"AAMC1\" name=\"ARROYO SECO\" originator=\"MTR\" generationtime=\"2026-03-10T00:02:00Z\">\n  <observed issued=\"2026-03-10T00:00:00Z\" primaryName=\"Stage\" primaryUnits=\"FT\">\n    <datum><valid>2026-03-10T00:00:00Z</valid><primary>2.5</primary></datum>\n  </observed>\n</site>\n",
        Some(body_extraction_plan(&[BodyExtractorId::WindHail])),
        classify_text_hml,
    );

    let ClassificationCandidate::Hml(candidate) = candidate.expect("expected hml candidate") else {
        panic!("expected hml candidate");
    };
    assert!(candidate.body_request.is_some());
}

#[test]
fn mos_classifier_carries_body_request_when_plan_is_enabled() {
    let candidate = with_specialized_context(
        "MET",
        "METBCK",
        "KBCK NAM MET GUIDANCE 03/10/2026 0000 UTC\nHR 00 03 06\nTMP 20 21 22\nWND 05 06 07\n",
        Some(body_extraction_plan(&[BodyExtractorId::WindHail])),
        classify_text_mos,
    );

    let ClassificationCandidate::Mos(candidate) = candidate.expect("expected mos candidate") else {
        panic!("expected mos candidate");
    };
    assert!(candidate.body_request.is_some());
}

#[test]
fn wmo_metar_strategy_returns_metar_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "SAGL31.TXT",
        b"000 \nSAGL31 BGGH 070200\nMETAR BGKK 070220Z AUTO VRB02KT 9999NDV OVC043/// M03/M08 Q0967=\n",
    ));

    assert!(matches!(
        classify(&envelope),
        ClassificationCandidate::Metar(_)
    ));
}

#[test]
fn wmo_taf_strategy_returns_taf_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "TAFWBCFJ.TXT",
        b"000 \nFTXX01 KWBC 070200\nTAF AMD\nWBCF 070244Z 0703/0803 18012KT P6SM SCT050\n",
    ));

    assert!(matches!(
        classify(&envelope),
        ClassificationCandidate::Taf(_)
    ));
}

#[test]
fn wmo_dcp_strategy_returns_dcp_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "MISDCPSV.TXT",
        b"SXMS50 KWAL 070258\n83786162 066025814\n16.23\n003\n137\n071\n088\n12.9\n137\n007\n00000\n 42-0NN  45E\n",
    ));

    assert!(matches!(
        classify(&envelope),
        ClassificationCandidate::Dcp(_)
    ));
}

#[test]
fn wmo_sigmet_strategy_returns_sigmet_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "WVID21.TXT",
        b"WVID21 WAAA 090100\nWAAF SIGMET 05 VALID 090100/090700 WAAA-\nWAAF UJUNG PANDANG FIR VA ERUPTION MT IBU=\n",
    ));

    assert!(matches!(
        classify(&envelope),
        ClassificationCandidate::Sigmet(_)
    ));
}

#[test]
fn wmo_airmet_returns_unsupported_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "WAAB31.TXT",
        b"WAAB31 LATI 090038\nLAAA AIRMET 1 VALID 090100/090500 LATI-\nLAAA TIRANA FIR MOD ICE=\n",
    ));

    assert!(matches!(
        classify(&envelope),
        ClassificationCandidate::UnsupportedWmo(_)
    ));
}

#[test]
fn wmo_canadian_text_returns_unsupported_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "FPCN11.TXT",
        b"FPCN11 CWWG 090059 AAD\nUPDATED FORECASTS FOR SOUTHERN MANITOBA ISSUED BY ENVIRONMENT CANADA\n",
    ));

    assert!(matches!(
        classify(&envelope),
        ClassificationCandidate::UnsupportedWmo(_)
    ));
}

#[test]
fn wmo_surface_observation_returns_unsupported_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "SAHOURLY.TXT",
        b"SACN74 CWAO 090000 RRC\n\nNPL SA 0000 AUTO8 M M M=\n",
    ));

    assert!(matches!(
        classify(&envelope),
        ClassificationCandidate::UnsupportedWmo(_)
    ));
}

#[test]
fn unknown_valid_wmo_returns_generic_unsupported_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "UNKNOWN.TXT",
        b"ABCD12 EFGH 090000\nSOME UNKNOWN BODY\n",
    ));

    let ClassificationCandidate::UnsupportedWmo(candidate) = classify(&envelope) else {
        panic!("expected unsupported WMO candidate");
    };

    assert_eq!(candidate.code, "unsupported_wmo_bulletin");
}

#[test]
fn failed_fd_parse_stays_with_fd_family() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "FDFAIL.TXT",
        b"000 \nSAGL31 BGGH 070200\nDATA BASED ON 070000Z\nVALID 071200Z\nFT 3000 6000\nMETAR BGKK 070220Z AUTO VRB02KT 9999NDV OVC043/// M03/M08 Q0967=\n",
    ));

    let ClassificationCandidate::MalformedFamily(candidate) = classify(&envelope) else {
        panic!("expected malformed-family candidate");
    };
    assert_eq!(candidate.family, "fd_bulletin");
    assert_eq!(candidate.issues[0].code, "invalid_fd_bulletin");
}
