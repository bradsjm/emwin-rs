mod common;

use common::{assert_family, assert_generic_body, enrich, fixture_cases};

fn assert_bodyless_text_product(case: &common::FixtureCase) {
    let enrichment = enrich(case);
    assert_family(&enrichment, "nws_text_product", case);
    assert!(
        enrichment.body.is_none(),
        "{} -> expected no extracted body",
        case.name
    );
}

#[test]
fn airport_weather_warning_corpus_uses_generic_body() {
    for case in fixture_cases("generic", "airport_weather_warning") {
        let enrichment = enrich(&case);
        assert_generic_body(&enrichment, &case);
        if case.name == "AWWUNVPA.TXT" {
            let body = enrichment
                .body
                .as_ref()
                .and_then(|body| body.as_generic())
                .unwrap_or_else(|| panic!("{} -> expected generic body", case.name));
            assert!(
                body.latlon
                    .as_ref()
                    .is_some_and(|polygons| !polygons.is_empty()),
                "{} -> expected LAT...LON geometry",
                case.name
            );
        }
    }
}

#[test]
fn regional_weather_roundup_corpus_uses_generic_body() {
    for case in fixture_cases("generic", "regional_weather_roundup") {
        assert_bodyless_text_product(&case);
    }
}

#[test]
fn tabular_state_forecast_corpus_uses_generic_body() {
    for case in fixture_cases("generic", "tabular_state_forecast") {
        assert_bodyless_text_product(&case);
    }
}

#[test]
fn gateway_observations_corpus_uses_generic_body() {
    for case in fixture_cases("generic", "gateway_observations") {
        assert_bodyless_text_product(&case);
    }
}
