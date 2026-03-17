mod common;

use common::{assert_vtec_body, enrich, fixture_cases};

#[test]
fn non_precipitation_warning_corpus_uses_vtec_event_body() {
    for case in fixture_cases("generic", "non_precipitation_warning") {
        let enrichment = enrich(&case);
        assert_vtec_body(&enrichment, &case);
        if case.name == "NPWLOXCA.TXT" {
            assert!(
                enrichment
                    .issues
                    .iter()
                    .all(|issue| issue.code != "vtec_segment_missing_required_polygon"),
                "{} -> expected zone-only NPW product to skip missing polygon QC",
                case.name
            );
        }
    }
}

#[test]
fn red_flag_warning_corpus_uses_vtec_event_body() {
    for case in fixture_cases("generic", "red_flag_warning") {
        let enrichment = enrich(&case);
        assert_vtec_body(&enrichment, &case);
    }
}

#[test]
fn winter_weather_message_corpus_uses_vtec_event_body() {
    for case in fixture_cases("generic", "winter_weather_message") {
        let enrichment = enrich(&case);
        assert_vtec_body(&enrichment, &case);
    }
}
