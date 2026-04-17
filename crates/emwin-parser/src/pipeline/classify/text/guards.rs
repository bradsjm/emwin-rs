use super::super::context::TextClassificationContext;
use crate::data::TextProductRouting;

use super::super::common::{
    filename_stem, first_nonempty_line, looks_like_multipart_taf_bulletin,
    starts_with_icao_sigmet_line,
};

pub(crate) fn matches_routed_text_family(
    context: &TextClassificationContext<'_>,
    routing: TextProductRouting,
    guard: fn(&str, &str) -> bool,
) -> bool {
    context.has_routing(routing) && guard(&context.header.afos, context.body_text)
}

/// Detects whether AFOS text resembles an FD bulletin.
pub(crate) fn looks_like_fd_text_product(afos: &str, body_text: &str) -> bool {
    matches!(
        afos.get(..3),
        Some("FD0" | "FD1" | "FD2" | "FD3" | "FD8" | "FD9" | "FDI")
    ) || body_text.contains("DATA BASED ON ")
        && body_text.contains("VALID ")
        && body_text
            .lines()
            .any(|line| line.trim_start().starts_with("FT "))
}

/// Detects whether a WMO-only bulletin resembles an FD bulletin.
pub(crate) fn looks_like_fd_wmo_bulletin(filename: &str, body_text: &str) -> bool {
    filename_stem(filename).starts_with("FD")
        && body_text.contains("DATA BASED ON ")
        && body_text.contains("VALID ")
        && body_text
            .lines()
            .any(|line| line.trim_start().starts_with("FT "))
}

/// Detects whether AFOS text resembles a PIREP bulletin.
pub(crate) fn looks_like_pirep_text_product(afos: &str, body_text: &str) -> bool {
    let trimmed = body_text.trim_start();
    let has_kind = trimmed.starts_with("UA ")
        || trimmed.starts_with("UUA ")
        || body_text.contains("\nUA ")
        || body_text.contains("\nUUA ")
        || body_text.contains(" UA ")
        || body_text.contains(" UUA ");
    afos.starts_with("PIR")
        || afos.eq_ignore_ascii_case("PRCUS")
        || afos.eq_ignore_ascii_case("PIREP")
        || body_text.trim_start().starts_with("ARP ")
        || ((body_text.contains("/OV ") || body_text.contains("/OV"))
            && body_text.contains("/TM")
            && has_kind)
}

/// Detects whether AFOS text resembles a CLI bulletin.
pub(crate) fn looks_like_cli_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("CLI")
        || (body_text.to_ascii_uppercase().contains("CLIMATE SUMMARY")
            && body_text.to_ascii_uppercase().contains("WEATHER ITEM"))
}

/// Detects whether AFOS text resembles a SIGMET bulletin.
pub(crate) fn looks_like_sigmet_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("SIG")
        || afos.starts_with("WS")
        || body_text.trim_start().starts_with("CONVECTIVE SIGMET ")
        || body_text.trim_start().starts_with("KZAK SIGMET ")
        || body_text.trim_start().starts_with("SIGMET ")
}

pub(crate) fn looks_like_lsr_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("LSR") && {
        let uppercase = body_text.to_ascii_uppercase();
        (uppercase.contains("..TIME..") && uppercase.contains("..DATE.."))
            || uppercase.contains("PRELIMINARY LOCAL STORM REPORT")
            || uppercase.contains("LOCAL STORM REPORT")
    }
}

pub(crate) fn looks_like_cwa_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("CWA")
        || (body_text.contains(" CWA ") && body_text.contains("VALID UNTIL"))
        || body_text
            .lines()
            .next()
            .is_some_and(|line| line.contains(" CWA "))
}

pub(crate) fn looks_like_wwp_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("WWP")
        && body_text.contains("PROBABILITY TABLE:")
        && (body_text.contains("ATTRIBUTE TABLE:")
            || body_text.contains("WATCH PROBABILITIES FOR WT")
            || body_text.contains("WATCH PROBABILITIES FOR WS"))
}

pub(crate) fn looks_like_saw_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("SAW") && body_text.contains("WW ") && body_text.contains("SPC AWW")
}

pub(crate) fn looks_like_sel_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("SEL") && body_text.contains("URGENT - IMMEDIATE BROADCAST REQUESTED") && {
        let uppercase = body_text.to_ascii_uppercase();
        uppercase.contains("WATCH NUMBER") || uppercase.contains("WATCH - NUMBER")
    }
}

pub(crate) fn looks_like_cf6_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("CF6")
        || (body_text.contains("PRELIMINARY LOCAL CLIMATOLOGICAL DATA")
            && body_text.contains("MONTH:")
            && body_text.contains("YEAR:"))
}

pub(crate) fn looks_like_dsm_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("DSM")
        || body_text.contains(" DS ")
            && body_text.lines().any(|line| {
                let trimmed = line.trim();
                trimmed.len() >= 7
                    && trimmed.as_bytes().get(4..7) == Some(b" DS")
                    && trimmed.contains('/')
            })
}

pub(crate) fn looks_like_hml_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("HML") && body_text.contains("<?xml")
}

pub(crate) fn looks_like_mos_text_product(afos: &str, body_text: &str) -> bool {
    matches!(
        afos.get(..3),
        Some("MET" | "MAV" | "MEX" | "FRH" | "FTP" | "ECS" | "LAV" | "LEV" | "NBE" | "NBS" | "NBX")
    ) && ((body_text.contains("GUIDANCE") && body_text.contains("MOS GUIDANCE"))
        || (body_text.contains("GUIDANCE")
            && body_text.lines().any(|line| {
                let trimmed = line.trim_start();
                trimmed.starts_with("HR")
                    || trimmed.starts_with("FHR")
                    || trimmed.starts_with("UTC")
            }))
        || body_text
            .lines()
            .any(|line| line.trim_start().starts_with(".B ")))
}

pub(crate) fn looks_like_mcd_text_product(afos: &str, body_text: &str) -> bool {
    matches!(afos, "SWOMCD" | "FFGMPD")
        || body_text.contains("MESOSCALE DISCUSSION")
        || body_text.contains("MPD")
        || body_text.contains("AREAS AFFECTED")
}

pub(crate) fn looks_like_ero_text_product(afos: &str, body_text: &str) -> bool {
    matches!(afos, "RBG94E" | "RBG98E" | "RBG99E")
        && body_text.contains("Excessive Rainfall")
        && body_text.contains("TO THE RIGHT OF A LINE FROM")
}

pub(crate) fn looks_like_spc_outlook_text_product(afos: &str, body_text: &str) -> bool {
    matches!(
        afos,
        "PTSDY1" | "PTSDY2" | "PTSDY3" | "PTSD48" | "PFWFD1" | "PFWFD2" | "PFWF38"
    ) && body_text.contains("VALID")
}

/// Detects whether WMO-only text resembles a SIGMET bulletin.
pub(crate) fn looks_like_sigmet_wmo_bulletin(body_text: &str) -> bool {
    let Some(first_line) = first_nonempty_line(body_text) else {
        return false;
    };
    first_line.starts_with("SIGMET ")
        || starts_with_icao_sigmet_line(first_line)
        || (first_line.contains(" SIGMET ") && first_line.contains(" VALID "))
}

pub(crate) fn looks_like_metar_wmo_bulletin(body_text: &str) -> bool {
    body_text.lines().map(str::trim).any(|line| {
        line == "METAR"
            || line == "SPECI"
            || line.starts_with("METAR ")
            || line.starts_with("SPECI ")
    })
}

pub(crate) fn looks_like_metar_text_product(afos: &str, body_text: &str) -> bool {
    matches!(afos, "METAR" | "SPECI") || looks_like_metar_wmo_bulletin(body_text)
}

pub(crate) fn looks_like_international_pirep_text_product(afos: &str, body_text: &str) -> bool {
    afos == "PIREP"
        && body_text.to_ascii_uppercase().contains(" OBSD AT ")
        && !body_text.contains('/')
}

pub(crate) fn looks_like_taf_wmo_bulletin(body_text: &str) -> bool {
    body_text.lines().map(str::trim).any(|line| {
        line == "TAF"
            || line.starts_with("TAF ")
            || (line.len() > 3
                && line.starts_with("TAF")
                && line
                    .chars()
                    .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit()))
    })
}

pub(crate) fn looks_like_taf_text_product(afos: &str, body_text: &str) -> bool {
    afos.starts_with("TAF")
        && (looks_like_taf_wmo_bulletin(body_text)
            || looks_like_station_led_taf_body(body_text)
            || looks_like_multipart_taf_bulletin(body_text))
}

pub(crate) fn looks_like_station_led_taf_body(body_text: &str) -> bool {
    let mut parts = body_text.split_whitespace();
    let Some(station) = parts.next() else {
        return false;
    };
    let Some(issue_time) = parts.next() else {
        return false;
    };
    (3..=4).contains(&station.len())
        && station.chars().all(|ch| ch.is_ascii_alphanumeric())
        && issue_time.len() == 7
        && issue_time.ends_with('Z')
        && issue_time[..6].chars().all(|ch| ch.is_ascii_digit())
}

/// Detects whether WMO-only text resembles an AIRMET bulletin.
pub(crate) fn looks_like_airmet_wmo_bulletin(body_text: &str) -> bool {
    first_nonempty_line(body_text)
        .is_some_and(|line| line.contains(" AIRMET ") && line.contains(" VALID "))
}

/// Detects Canadian Environment Canada text bulletins.
pub(crate) fn looks_like_canadian_text_bulletin(
    header: &crate::WmoHeader,
    body_text: &str,
) -> bool {
    header.cccc.starts_with("CW") || body_text.contains("ENVIRONMENT CANADA")
}

/// Detects unsupported surface observation bulletins.
pub(crate) fn looks_like_surface_observation_bulletin(body_text: &str) -> bool {
    first_nonempty_line(body_text).is_some_and(|line| line.starts_with("NPL SA "))
}
