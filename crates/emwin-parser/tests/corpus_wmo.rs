mod common;

use common::{
    assert_family, assert_supported_family, assert_wmo, enrich, fixture_cases, issue_codes,
};
use emwin_parser::{TafForecastGroupKind, enrich_product};

#[test]
fn metar_corpus_routes_to_wmo_bulletins() {
    for case in fixture_cases("wmo", "metar_collective") {
        let enrichment = enrich(&case);
        assert_supported_family(
            &enrichment,
            "metar_collective",
            &case,
            &["invalid_metar_bulletin", "invalid_metar_report"],
        );
        let Some(artifact) = enrichment.parsed.as_ref() else {
            continue;
        };
        let bulletin = artifact
            .as_metar()
            .unwrap_or_else(|| panic!("{} -> expected METAR artifact", case.name));
        assert!(
            !bulletin.reports.is_empty(),
            "{} -> expected METAR reports",
            case.name
        );
        for report in &bulletin.reports {
            assert!(
                !report.station.trim().is_empty(),
                "{} -> expected METAR station",
                case.name
            );
            assert!(
                !report.observation_time.trim().is_empty(),
                "{} -> expected METAR observation time",
                case.name
            );
            assert!(
                !report.raw.trim().is_empty(),
                "{} -> expected preserved METAR report text",
                case.name
            );
            if let Some(wind) = &report.wind
                && let Some(gust_kt) = wind.gust_kt
            {
                assert!(
                    gust_kt >= wind.speed_kt,
                    "{} -> METAR gust must be at least steady wind",
                    case.name
                );
            }
            if let Some(altimeter) = &report.altimeter {
                assert!(
                    !altimeter.trim().is_empty(),
                    "{} -> expected non-empty METAR altimeter token",
                    case.name
                );
            }
        }
    }
}

#[test]
fn taf_corpus_routes_to_wmo_bulletins() {
    for case in fixture_cases("wmo", "taf_bulletin") {
        let enrichment = enrich(&case);
        assert_supported_family(
            &enrichment,
            "taf_bulletin",
            &case,
            &["invalid_taf_bulletin"],
        );
        let Some(artifact) = enrichment.parsed.as_ref() else {
            continue;
        };
        let bulletin = artifact
            .as_taf()
            .unwrap_or_else(|| panic!("{} -> expected TAF artifact", case.name));
        assert!(
            !bulletin.station.trim().is_empty(),
            "{} -> expected station",
            case.name
        );
        assert!(
            !bulletin.issue_time.trim().is_empty(),
            "{} -> expected TAF issue time",
            case.name
        );
        assert!(
            !bulletin.raw.trim().is_empty(),
            "{} -> expected preserved TAF text",
            case.name
        );
        assert_eq!(
            bulletin.valid_from.is_some(),
            bulletin.valid_to.is_some(),
            "{} -> TAF validity bounds must be paired",
            case.name
        );
        for group in &bulletin.groups {
            assert!(
                !group.raw.trim().is_empty(),
                "{} -> expected TAF group raw text",
                case.name
            );
            if let Some(probability_percent) = group.probability_percent {
                assert!(
                    matches!(
                        group.change_kind,
                        TafForecastGroupKind::Prob
                            | TafForecastGroupKind::Tempo
                            | TafForecastGroupKind::Inter
                    ),
                    "{} -> only PROB, TEMPO, or INTER groups may carry probability",
                    case.name
                );
                assert!(
                    probability_percent <= 100,
                    "{} -> invalid TAF probability",
                    case.name
                );
            }
        }
    }
}

#[test]
fn sigmet_corpus_routes_to_wmo_bulletins() {
    for case in fixture_cases("wmo", "sigmet_bulletin") {
        let enrichment = enrich(&case);
        if enrichment.family == Some("international_sigmet_bulletin") {
            assert!(
                issue_codes(&enrichment.issues)
                    .contains("unsupported_international_sigmet_bulletin"),
                "{} -> expected unsupported international SIGMET issue",
                case.name
            );
            continue;
        }
        assert_supported_family(
            &enrichment,
            "sigmet_bulletin",
            &case,
            &["invalid_sigmet_bulletin"],
        );
        let Some(artifact) = enrichment.parsed.as_ref() else {
            continue;
        };
        let bulletin = artifact
            .as_sigmet()
            .unwrap_or_else(|| panic!("{} -> expected SIGMET artifact", case.name));
        assert!(
            !bulletin.sections.is_empty(),
            "{} -> expected SIGMET sections",
            case.name
        );
        assert!(
            bulletin
                .sections
                .iter()
                .any(|section| !section.raw.trim().is_empty()),
            "{} -> expected populated SIGMET sections",
            case.name
        );
    }
}

#[test]
fn fd_corpus_routes_to_wmo_bulletins() {
    for case in fixture_cases("wmo", "fd_bulletin") {
        let enrichment = enrich(&case);
        assert_supported_family(
            &enrichment,
            "fd_bulletin",
            &case,
            &["invalid_fd_bulletin", "missing_reference_time"],
        );
        let Some(artifact) = enrichment.parsed.as_ref() else {
            continue;
        };
        let bulletin = artifact
            .as_fd()
            .unwrap_or_else(|| panic!("{} -> expected FD artifact", case.name));
        assert!(
            !bulletin.forecasts.is_empty(),
            "{} -> expected FD forecasts",
            case.name
        );
        assert!(
            !bulletin.based_on_time.trim().is_empty(),
            "{} -> expected FD based-on time",
            case.name
        );
        assert!(
            !bulletin.valid_time.trim().is_empty(),
            "{} -> expected FD valid time",
            case.name
        );
        assert!(
            !bulletin.levels.is_empty(),
            "{} -> expected FD levels",
            case.name
        );
        for forecast in &bulletin.forecasts {
            assert!(
                !forecast.station.trim().is_empty(),
                "{} -> expected FD station",
                case.name
            );
            assert!(
                !forecast.groups.is_empty(),
                "{} -> expected FD forecast groups",
                case.name
            );
            for group in &forecast.groups {
                assert!(
                    group.altitude_hundreds_ft > 0,
                    "{} -> expected positive FD altitude",
                    case.name
                );
                if group.wind_direction_degrees.is_some() {
                    assert!(
                        group.wind_speed_kt.is_some(),
                        "{} -> FD wind direction requires speed",
                        case.name
                    );
                }
            }
        }
    }
}

#[test]
fn dcp_corpus_routes_to_wmo_bulletins() {
    for case in fixture_cases("wmo", "dcp_telemetry_bulletin") {
        let filename = if case.name == "SXMS50.TXT" {
            "MISDCPSV.TXT"
        } else {
            &case.name
        };
        let enrichment = enrich_product(filename, &case.bytes);
        let artifact = assert_wmo(&enrichment, "dcp_telemetry_bulletin", &case, &[]);
        let bulletin = artifact
            .as_dcp()
            .unwrap_or_else(|| panic!("{} -> expected DCP artifact", case.name));
        assert!(
            bulletin.platform_id.is_some() || !bulletin.lines.is_empty(),
            "{} -> expected DCP payload",
            case.name
        );
    }
}

fn assert_explicit_unsupported_family(family: &str, issue_code: &str, title: &str) {
    for case in fixture_cases("wmo", family) {
        let enrichment = enrich(&case);
        assert_family(&enrichment, family, &case);
        assert_eq!(
            enrichment.title,
            Some(title),
            "{} -> expected title {title}, got {:?}",
            case.name,
            enrichment.title
        );
        assert!(
            enrichment.parsed.is_none(),
            "{} -> expected no parsed artifact",
            case.name
        );
        assert!(
            issue_codes(&enrichment.issues).contains(issue_code),
            "{} -> expected issue code {issue_code}, got {:?}",
            case.name,
            issue_codes(&enrichment.issues)
        );
    }
}

#[test]
fn canadian_tornado_warning_corpus_routes_to_explicit_unsupported_family() {
    assert_explicit_unsupported_family(
        "canadian_tornado_warning_bulletin",
        "unsupported_canadian_tornado_warning_bulletin",
        "Canadian tornado warning bulletin",
    );
}

#[test]
fn canadian_severe_thunderstorm_warning_corpus_routes_to_explicit_unsupported_family() {
    assert_explicit_unsupported_family(
        "canadian_severe_thunderstorm_warning_bulletin",
        "unsupported_canadian_severe_thunderstorm_warning_bulletin",
        "Canadian severe thunderstorm warning bulletin",
    );
}

#[test]
fn canadian_tropical_cyclone_public_information_corpus_routes_to_explicit_unsupported_family() {
    assert_explicit_unsupported_family(
        "canadian_tropical_cyclone_public_information",
        "unsupported_canadian_tropical_cyclone_public_information",
        "Canadian tropical cyclone public information",
    );
}

#[test]
fn canadian_tropical_cyclone_watch_warning_corpus_routes_to_explicit_unsupported_family() {
    assert_explicit_unsupported_family(
        "canadian_tropical_cyclone_watch_warning_bulletin",
        "unsupported_canadian_tropical_cyclone_watch_warning_bulletin",
        "Canadian tropical cyclone watch/warning bulletin",
    );
}

#[test]
fn canadian_tropical_cyclone_technical_discussion_corpus_routes_to_explicit_unsupported_family() {
    assert_explicit_unsupported_family(
        "canadian_tropical_cyclone_technical_discussion",
        "unsupported_canadian_tropical_cyclone_technical_discussion",
        "Canadian tropical cyclone technical discussion",
    );
}

#[test]
fn canadian_storm_summary_corpus_routes_to_explicit_unsupported_family() {
    assert_explicit_unsupported_family(
        "canadian_storm_summary",
        "unsupported_canadian_storm_summary",
        "Canadian storm summary",
    );
}

#[test]
fn canadian_special_weather_statement_corpus_routes_to_explicit_unsupported_family() {
    assert_explicit_unsupported_family(
        "canadian_special_weather_statement",
        "unsupported_canadian_special_weather_statement",
        "Canadian special weather statement",
    );
}

#[test]
fn canadian_volcanic_ash_corpus_routes_to_explicit_unsupported_family() {
    assert_explicit_unsupported_family(
        "canadian_volcanic_ash_bulletin",
        "unsupported_canadian_volcanic_ash_bulletin",
        "Canadian volcanic ash bulletin",
    );
}
