use super::*;

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
