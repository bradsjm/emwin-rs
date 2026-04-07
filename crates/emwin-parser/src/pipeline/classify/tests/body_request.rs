use super::{
    BbbKind, BodyExtractorId, ClassificationCandidate, FdCandidate, NormalizedInput,
    ParsedEnvelope, ProductEnrichmentSource, TextProductHeader, TimeZone, Utc,
    body_extraction_plan, build_body_request, classify, classify_text_cf6, classify_text_cwa,
    classify_text_dsm, classify_text_hml, classify_text_lsr, classify_text_mos, classify_text_wwp,
    with_specialized_context,
};

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
