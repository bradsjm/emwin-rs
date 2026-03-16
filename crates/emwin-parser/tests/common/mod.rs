#![allow(dead_code)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use emwin_parser::{
    ProductArtifact, ProductBody, ProductEnrichment, ProductParseIssue, enrich_product,
};

#[derive(Debug, Clone)]
pub struct FixtureCase {
    pub path: PathBuf,
    pub name: String,
    pub bytes: Vec<u8>,
}

pub fn fixture_cases(domain: &str, family: &str) -> Vec<FixtureCase> {
    let root = fixture_dir(domain, family);
    let mut cases = Vec::new();
    collect_files(&root, &mut cases);
    cases.sort_by(|left, right| left.name.cmp(&right.name));
    assert!(
        !cases.is_empty(),
        "expected fixtures under {}",
        root.display()
    );
    cases
}

pub fn enrich(case: &FixtureCase) -> ProductEnrichment {
    enrich_product(&case.name, &case.bytes)
}

pub fn issue_codes(issues: &[ProductParseIssue]) -> BTreeSet<&str> {
    issues.iter().map(|issue| issue.code).collect()
}

pub fn has_any_issue(issues: &[ProductParseIssue], codes: &[&str]) -> bool {
    issues.iter().any(|issue| codes.contains(&issue.code))
}

pub fn matches_any(name: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|pattern| name.contains(pattern))
}

pub fn assert_family(enrichment: &ProductEnrichment, expected: &str, case: &FixtureCase) {
    assert_eq!(
        enrichment.family,
        Some(expected),
        "{} -> expected family {expected}, got {:?}",
        case.name,
        enrichment.family
    );
}

pub fn assert_specialized<'a>(
    enrichment: &'a ProductEnrichment,
    expected: &str,
    case: &FixtureCase,
    parse_error_allowlist: &[&str],
) -> &'a ProductArtifact {
    assert_family(enrichment, expected, case);
    assert!(
        enrichment.body.is_none() || enrichment.body.as_ref().is_some_and(has_body_content),
        "{} -> expected optional body content to be non-empty when present: {:#?}",
        case.name,
        enrichment.body
    );
    assert!(
        enrichment.parsed.is_some() || has_any_issue(&enrichment.issues, parse_error_allowlist),
        "{} -> expected parsed artifact or allowlisted issue, issues={:?}",
        case.name,
        issue_codes(&enrichment.issues)
    );
    enrichment.parsed.as_ref().unwrap_or_else(|| {
        panic!(
            "{} -> missing parsed artifact without allowlisted issue, issues={:#?}",
            case.name, enrichment.issues
        )
    })
}

pub fn assert_supported_family(
    enrichment: &ProductEnrichment,
    expected: &str,
    case: &FixtureCase,
    allow_issues: &[&str],
) {
    assert_family(enrichment, expected, case);
    assert!(
        enrichment.parsed.is_some() || has_any_issue(&enrichment.issues, allow_issues),
        "{} -> expected parsed artifact or allowlisted degraded issues, issues={:?}",
        case.name,
        issue_codes(&enrichment.issues)
    );
}

pub fn assert_wmo<'a>(
    enrichment: &'a ProductEnrichment,
    expected: &str,
    case: &FixtureCase,
    parse_error_allowlist: &[&str],
) -> &'a ProductArtifact {
    assert_family(enrichment, expected, case);
    assert!(
        enrichment.parsed.is_some() || has_any_issue(&enrichment.issues, parse_error_allowlist),
        "{} -> expected parsed WMO artifact or allowlisted issue, issues={:?}",
        case.name,
        issue_codes(&enrichment.issues)
    );
    enrichment.parsed.as_ref().unwrap_or_else(|| {
        panic!(
            "{} -> missing parsed WMO artifact without allowlisted issue, issues={:#?}",
            case.name, enrichment.issues
        )
    })
}

pub fn assert_vtec_body(enrichment: &ProductEnrichment, case: &FixtureCase) {
    assert_family(enrichment, "nws_text_product", case);
    let body = enrichment
        .body
        .as_ref()
        .unwrap_or_else(|| panic!("{} -> expected body", case.name));
    let vtec = body
        .as_vtec_event()
        .unwrap_or_else(|| panic!("{} -> expected vtec event body, got {body:#?}", case.name));
    assert!(
        !vtec.segments.is_empty(),
        "{} -> expected at least one VTEC segment",
        case.name
    );
    assert!(
        vtec.segments.iter().all(|segment| !segment.vtec.is_empty()),
        "{} -> expected every segment to carry VTEC codes: {vtec:#?}",
        case.name
    );
}

pub fn assert_generic_body(enrichment: &ProductEnrichment, case: &FixtureCase) {
    assert_family(enrichment, "nws_text_product", case);
    let body = enrichment
        .body
        .as_ref()
        .unwrap_or_else(|| panic!("{} -> expected body", case.name));
    let generic = body
        .as_generic()
        .unwrap_or_else(|| panic!("{} -> expected generic body, got {body:#?}", case.name));
    assert!(
        generic.ugc.is_some()
            || generic.latlon.is_some()
            || generic.time_mot_loc.is_some()
            || generic.wind_hail.is_some(),
        "{} -> expected generic body content: {generic:#?}",
        case.name
    );
}

fn fixture_dir(domain: &str, family: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("products")
        .join(domain)
        .join(family)
}

fn collect_files(root: &Path, cases: &mut Vec<FixtureCase>) {
    let entries = fs::read_dir(root).unwrap_or_else(|error| {
        panic!(
            "failed to read fixture directory {}: {error}",
            root.display()
        )
    });
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| panic!("failed to read directory entry: {error}"));
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, cases);
            continue;
        }
        let name = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('/', "__");
        let bytes = fs::read(&path)
            .unwrap_or_else(|error| panic!("failed to read fixture {}: {error}", path.display()));
        cases.push(FixtureCase { path, name, bytes });
    }
}

fn has_body_content(body: &ProductBody) -> bool {
    match body {
        ProductBody::VtecEvent(body) => !body.segments.is_empty(),
        ProductBody::Generic(body) => {
            body.ugc.is_some()
                || body.latlon.is_some()
                || body.time_mot_loc.is_some()
                || body.wind_hail.is_some()
        }
    }
}
