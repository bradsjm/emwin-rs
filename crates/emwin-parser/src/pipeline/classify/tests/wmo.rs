use super::*;

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

    let ClassificationCandidate::UnsupportedWmo(candidate) = classify(&envelope) else {
        panic!("expected unsupported WMO candidate");
    };

    assert_eq!(candidate.code, "unsupported_international_sigmet_bulletin");
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
fn wmo_international_pirep_returns_unsupported_candidate() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "PIREP.TXT",
        b"UAJP71 RJFF 171210\n\nPIREP MOD TURB OBSD AT 1210 SANOR F340 REPORTED BY A320\n",
    ));

    let ClassificationCandidate::UnsupportedWmo(candidate) = classify(&envelope) else {
        panic!("expected unsupported WMO candidate");
    };

    assert_eq!(candidate.code, "unsupported_international_pirep_bulletin");
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
