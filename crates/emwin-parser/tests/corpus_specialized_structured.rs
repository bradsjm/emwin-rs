mod common;

use common::{assert_specialized, assert_supported_family, enrich, fixture_cases};
use emwin_parser::SpcOutlookFormat;

#[test]
fn cf6_corpus_routes_to_structured_bulletins() {
    for case in fixture_cases("specialized", "cf6") {
        let enrichment = enrich(&case);
        assert_supported_family(
            &enrichment,
            "cf6_bulletin",
            &case,
            &["invalid_cf6_bulletin"],
        );
        let Some(artifact) = enrichment.parsed.as_ref() else {
            continue;
        };
        let bulletin = artifact
            .as_cf6()
            .unwrap_or_else(|| panic!("{} -> expected CF6 artifact", case.name));
        assert!(
            !bulletin.station.trim().is_empty(),
            "{} -> expected station",
            case.name
        );
        assert!(
            (1..=12).contains(&bulletin.month),
            "{} -> expected month",
            case.name
        );
        assert!(
            !bulletin.rows.is_empty(),
            "{} -> expected day rows",
            case.name
        );
    }
}

#[test]
fn dsm_corpus_routes_to_structured_bulletins() {
    for case in fixture_cases("specialized", "dsm") {
        let enrichment = enrich(&case);
        assert_supported_family(
            &enrichment,
            "dsm_bulletin",
            &case,
            &["invalid_dsm_bulletin"],
        );
        let Some(artifact) = enrichment.parsed.as_ref() else {
            continue;
        };
        let bulletin = artifact
            .as_dsm()
            .unwrap_or_else(|| panic!("{} -> expected DSM artifact", case.name));
        assert!(
            !bulletin.summaries.is_empty(),
            "{} -> expected DSM summaries",
            case.name
        );
        for summary in &bulletin.summaries {
            assert!(
                !summary.station.trim().is_empty(),
                "{} -> expected DSM station",
                case.name
            );
            assert!(
                !summary.date.trim().is_empty(),
                "{} -> expected DSM date",
                case.name
            );
            assert_eq!(
                summary.hourly_precip_inches.len(),
                24,
                "{} -> expected 24 hourly precipitation values",
                case.name
            );
            if summary.max_wind_time.is_some() {
                assert!(
                    summary.max_wind_mph.is_some(),
                    "{} -> DSM max wind time requires max wind speed",
                    case.name
                );
            }
            if summary.max_gust_time.is_some() {
                assert!(
                    summary.max_gust_mph.is_some(),
                    "{} -> DSM max gust time requires max gust speed",
                    case.name
                );
            }
            if summary.max_wind_dir_degrees.is_some() {
                assert!(
                    summary.max_wind_mph.is_some(),
                    "{} -> DSM max wind direction requires max wind speed",
                    case.name
                );
            }
        }
    }
}

#[test]
fn hml_corpus_routes_to_structured_bulletins() {
    for case in fixture_cases("specialized", "hml") {
        let enrichment = enrich(&case);
        let artifact = assert_specialized(&enrichment, "hml_bulletin", &case, &[]);
        let bulletin = artifact
            .as_hml()
            .unwrap_or_else(|| panic!("{} -> expected HML artifact", case.name));
        assert!(
            !bulletin.documents.is_empty(),
            "{} -> expected HML documents",
            case.name
        );
    }
}

#[test]
fn ero_corpus_routes_to_structured_bulletins() {
    for case in fixture_cases("specialized", "ero") {
        let enrichment = enrich(&case);
        let artifact = assert_specialized(&enrichment, "ero_bulletin", &case, &[]);
        let bulletin = artifact
            .as_ero()
            .unwrap_or_else(|| panic!("{} -> expected ERO artifact", case.name));
        assert!(
            !bulletin.outlooks.is_empty(),
            "{} -> expected ERO outlooks",
            case.name
        );
    }
}

#[test]
fn spc_outlook_corpus_routes_to_structured_bulletins() {
    for case in fixture_cases("specialized", "spc_outlook") {
        let enrichment = enrich(&case);
        assert_supported_family(
            &enrichment,
            "spc_outlook_bulletin",
            &case,
            &["invalid_spc_outlook_bulletin"],
        );
        let Some(artifact) = enrichment.parsed.as_ref() else {
            continue;
        };
        let bulletin = artifact
            .as_spc_outlook()
            .unwrap_or_else(|| panic!("{} -> expected SPC outlook artifact", case.name));
        assert!(
            !bulletin.days.is_empty(),
            "{} -> expected SPC days",
            case.name
        );
        match bulletin.format {
            SpcOutlookFormat::Points => assert!(
                bulletin.days.iter().all(|day| !day.outlooks.is_empty()),
                "{} -> expected day outlooks",
                case.name
            ),
            SpcOutlookFormat::ArealOutline => {}
        }
    }
}
