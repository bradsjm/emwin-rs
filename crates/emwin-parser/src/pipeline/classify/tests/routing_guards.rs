use super::{
    BodyExtractorId, ClassificationCandidate, NormalizedInput, ParsedEnvelope,
    TextClassificationContext, TextProductHeader, Utc, body_extraction_plan, classify,
    classify_text_cf6, classify_text_dsm, classify_text_fd, classify_text_lsr, classify_text_mos,
    classify_text_pirep, classify_text_saw, classify_text_sel, resolved_text_product_policy,
    with_specialized_context,
};

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
