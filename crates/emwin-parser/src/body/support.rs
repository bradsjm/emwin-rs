//! Shared parsing helpers for body submodules.
//!
//! This module intentionally stays small and generic. It only contains marker
//! scanning and WKT formatting utilities that are used by multiple body
//! parsers; domain-specific parsing stays in the owning parser module.

use memchr::memchr_iter;

/// Finds the first ASCII case-insensitive occurrence of `needle` in `haystack`.
pub(crate) fn ascii_find_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }

    let haystack_bytes = haystack.as_bytes();
    let needle_bytes = needle.as_bytes();
    if needle_bytes.len() > haystack_bytes.len() {
        return None;
    }

    haystack_bytes
        .windows(needle_bytes.len())
        .position(|window| window.eq_ignore_ascii_case(needle_bytes))
}

/// Collects slash-delimited candidates that do not cross line boundaries.
pub(crate) fn scan_slash_delimited_candidates<F>(text: &str, predicate: F) -> Vec<&str>
where
    F: Fn(&str) -> bool,
{
    let bytes = text.as_bytes();
    let mut candidates = Vec::new();

    for start in memchr_iter(b'/', bytes) {
        let rest = &text[start..];
        let Some(end_rel) = rest
            .as_bytes()
            .iter()
            .enumerate()
            .skip(1)
            .find_map(|(index, byte)| match byte {
                b'/' => Some(index),
                b'\r' | b'\n' => None,
                _ => None,
            })
        else {
            continue;
        };

        let candidate = &text[start..=start + end_rel];
        if predicate(candidate) {
            candidates.push(candidate);
        }
    }

    candidates
}

/// Formats a WKT `LINESTRING` or `POINT` without intermediate `Vec<String>`.
pub(crate) fn format_linestring_wkt(points: &[(f64, f64)]) -> String {
    if points.len() == 1 {
        return format!("POINT({:.4} {:.4})", points[0].1, points[0].0);
    }

    let mut wkt = String::from("LINESTRING(");
    for (index, (lat, lon)) in points.iter().enumerate() {
        if index > 0 {
            wkt.push_str(", ");
        }
        use std::fmt::Write as _;
        let _ = write!(wkt, "{lon:.4} {lat:.4}");
    }
    wkt.push(')');
    wkt
}

/// Formats a WKT `POLYGON` without intermediate `Vec<String>`.
pub(crate) fn format_polygon_wkt(points: &[(f64, f64)]) -> String {
    let mut wkt = String::from("POLYGON((");
    for (index, (lat, lon)) in points.iter().enumerate() {
        if index > 0 {
            wkt.push_str(", ");
        }
        use std::fmt::Write as _;
        let _ = write!(wkt, "{lon:.6} {lat:.6}");
    }
    wkt.push_str("))");
    wkt
}
