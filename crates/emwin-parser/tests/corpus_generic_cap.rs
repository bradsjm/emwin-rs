mod common;

use common::{assert_family, assert_generic_body, assert_vtec_body, enrich, fixture_cases};

#[test]
fn cap_xml_corpus_preserves_text_identity_and_projects_body() {
    for case in fixture_cases("generic", "cap_xml") {
        let enrichment = enrich(&case);
        assert_family(&enrichment, "nws_text_product", &case);
        assert_eq!(
            enrichment.pil.as_deref(),
            Some("CAP"),
            "{} -> expected CAP PIL",
            case.name
        );
        assert!(
            enrichment.parsed.is_none(),
            "{} -> expected CAP to remain a generic-body projection",
            case.name
        );

        if case
            .bytes
            .windows(b"<valueName>VTEC</valueName>".len())
            .any(|window| window == b"<valueName>VTEC</valueName>")
        {
            assert_vtec_body(&enrichment, &case);
        } else {
            assert_generic_body(&enrichment, &case);
        }
    }
}

#[test]
fn cap_xml_corpus_projects_expected_spot_checks() {
    for case in fixture_cases("generic", "cap_xml") {
        let enrichment = enrich(&case);

        match case.name.as_str() {
            "CAPMEG.TXT" => {
                let body = enrichment
                    .body
                    .as_ref()
                    .and_then(|body| body.as_vtec_event())
                    .unwrap_or_else(|| panic!("{} -> expected vtec event body", case.name));
                assert!(
                    body.segments
                        .iter()
                        .any(|segment| { segment.vtec.iter().any(|code| code.action == "CAN") }),
                    "{} -> expected a cancel VTEC action",
                    case.name
                );
            }
            "CAPPAH.TXT" => {
                let body = enrichment
                    .body
                    .as_ref()
                    .and_then(|body| body.as_vtec_event())
                    .unwrap_or_else(|| panic!("{} -> expected vtec event body", case.name));
                let segment = body
                    .segments
                    .first()
                    .unwrap_or_else(|| panic!("{} -> expected at least one segment", case.name));
                assert!(
                    !segment.polygons.is_empty(),
                    "{} -> expected projected CAP polygon",
                    case.name
                );
                assert!(
                    !segment.time_mot_loc.is_empty(),
                    "{} -> expected projected event motion",
                    case.name
                );
            }
            "CAPWBC.TXT" => {
                assert!(
                    enrichment
                        .issues
                        .iter()
                        .all(|issue| issue.code != "cap_mixed_projection_omitted"),
                    "{} -> expected no mixed-projection issue for keepalive CAP",
                    case.name
                );
            }
            _ => {}
        }
    }
}
