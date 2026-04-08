use super::{ClassificationCandidate, NormalizedInput, ParsedEnvelope, classify};

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
fn afos_international_pirep_routes_to_explicit_unsupported_family() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "PIREP.TXT",
        b"000 \nUAJP71 RJFF 072253\nPIREP\nMOD TURB OBSD AT 2253 20NM N OF LESKA F300 REPORTED BY A320\n",
    ));

    let ClassificationCandidate::MalformedFamily(candidate) = classify(&envelope) else {
        panic!("expected malformed family candidate");
    };

    assert_eq!(candidate.family, "international_pirep_bulletin");
    assert_eq!(
        candidate.issues[0].code,
        "unsupported_international_pirep_bulletin"
    );
}

#[test]
fn afos_multipart_taf_routes_to_explicit_unsupported_family() {
    let envelope = ParsedEnvelope::build(NormalizedInput::from_input(
        "TAFMD1.TXT",
        b"000 \nFTXX99 KWBC 072303 RRB\nTAFMD1\nPART 1 OF 2 TAF SBBE 072110Z 0800/0824 05005KT 9999 FEW040\n",
    ));

    let ClassificationCandidate::MalformedFamily(candidate) = classify(&envelope) else {
        panic!("expected malformed family candidate");
    };

    assert_eq!(candidate.family, "multipart_taf_bulletin");
    assert_eq!(
        candidate.issues[0].code,
        "unsupported_multipart_taf_bulletin"
    );
}
