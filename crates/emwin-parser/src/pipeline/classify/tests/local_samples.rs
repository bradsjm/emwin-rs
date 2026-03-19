use super::*;

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
