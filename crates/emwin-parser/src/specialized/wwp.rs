//! Parsing for SPC watch probability products.

use regex::Regex;
use serde::Serialize;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpcWatchType {
    Tornado,
    SevereThunderstorm,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WwpBulletin {
    pub watch_type: SpcWatchType,
    pub watch_number: u16,
    pub prob_tornadoes_2_or_more: u8,
    pub prob_tornadoes_1_or_more_strong: u8,
    pub prob_severe_wind_10_or_more: u8,
    pub prob_wind_1_or_more_65kt: u8,
    pub prob_severe_hail_10_or_more: u8,
    pub prob_hail_1_or_more_2inch: u8,
    pub prob_combined_hail_wind_6_or_more: u8,
    pub max_hail_inches: f32,
    pub max_wind_gust_knots: u16,
    pub max_tops_feet: u32,
    pub storm_motion_degrees: u16,
    pub storm_motion_knots: u16,
    pub is_pds: bool,
}

pub(crate) fn parse_wwp_bulletin(text: &str) -> Option<WwpBulletin> {
    let normalized = text.replace('\r', "");
    let header = header_re().captures(&normalized)?;
    let prob = prob_re().captures(&normalized)?;
    let attr = attr_re().captures(&normalized)?;
    Some(WwpBulletin {
        watch_type: if header.name("typ")?.as_str() == "TORNADO" {
            SpcWatchType::Tornado
        } else {
            SpcWatchType::SevereThunderstorm
        },
        watch_number: header.name("num")?.as_str().parse().ok()?,
        prob_tornadoes_2_or_more: parse_prob(prob.name("t2")?.as_str())?,
        prob_tornadoes_1_or_more_strong: parse_prob(prob.name("t1s")?.as_str())?,
        prob_severe_wind_10_or_more: parse_prob(prob.name("w10")?.as_str())?,
        prob_wind_1_or_more_65kt: parse_prob(prob.name("w1")?.as_str())?,
        prob_severe_hail_10_or_more: parse_prob(prob.name("h10")?.as_str())?,
        prob_hail_1_or_more_2inch: parse_prob(prob.name("h1")?.as_str())?,
        prob_combined_hail_wind_6_or_more: parse_prob(prob.name("hw6")?.as_str())?,
        max_hail_inches: attr.name("hail")?.as_str().parse().ok()?,
        max_wind_gust_knots: attr.name("wind")?.as_str().parse().ok()?,
        max_tops_feet: attr.name("tops")?.as_str().parse::<u32>().ok()? * 100,
        storm_motion_degrees: attr.name("dir")?.as_str().parse().ok()?,
        storm_motion_knots: attr.name("spd")?.as_str().parse().ok()?,
        is_pds: attr.name("pds")?.as_str() == "YES",
    })
}

fn header_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?m)^(?P<typ>TORNADO|SEVERE THUNDERSTORM) WATCH PROBABILITIES FOR W[TS] (?P<num>\d{4})$",
        )
        .expect("valid WWP header regex")
    })
}

fn prob_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"PROB OF 2 OR MORE TORNADOES\s+:\s+(?P<t2>[<>\d]+)%\s+PROB OF 1 OR MORE STRONG /EF2-EF5/ TORNADOES\s+:\s+(?P<t1s>[<>\d]+)%\s+PROB OF 10 OR MORE SEVERE WIND EVENTS\s+:\s+(?P<w10>[<>\d]+)%\s+PROB OF 1 OR MORE WIND EVENTS >= 65 KNOTS\s+:\s+(?P<w1>[<>\d]+)%\s+PROB OF 10 OR MORE SEVERE HAIL EVENTS\s+:\s+(?P<h10>[<>\d]+)%\s+PROB OF 1 OR MORE HAIL EVENTS >= 2 INCHES\s+:\s+(?P<h1>[<>\d]+)%\s+PROB OF 6 OR MORE COMBINED SEVERE HAIL/WIND EVENTS\s+:\s+(?P<hw6>[<>\d]+)%",
        )
        .expect("valid WWP probability regex")
    })
}

fn attr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"MAX HAIL /INCHES/\s+:\s+(?P<hail>[\d.]+)\s+MAX WIND GUSTS SURFACE /KNOTS/\s+:\s+(?P<wind>\d+)\s+MAX TOPS /X 100 FEET/\s+:\s+(?P<tops>\d+)\s+MEAN STORM MOTION VECTOR /DEGREES AND KNOTS/\s+:\s+(?P<dir>\d{3})(?P<spd>\d{2})\s+PARTICULARLY DANGEROUS SITUATION\s+:\s+(?P<pds>YES|NO)",
        )
        .expect("valid WWP attr regex")
    })
}

fn parse_prob(value: &str) -> Option<u8> {
    value.trim_matches(&['<', '>'][..]).parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{SpcWatchType, parse_wwp_bulletin};

    #[test]
    fn parses_local_wwp_bulletin() {
        let text = "\
TORNADO WATCH PROBABILITIES FOR WT 0031

PROB OF 2 OR MORE TORNADOES : 20%
PROB OF 1 OR MORE STRONG /EF2-EF5/ TORNADOES : 10%
PROB OF 10 OR MORE SEVERE WIND EVENTS : 70%
PROB OF 1 OR MORE WIND EVENTS >= 65 KNOTS : 40%
PROB OF 10 OR MORE SEVERE HAIL EVENTS : 60%
PROB OF 1 OR MORE HAIL EVENTS >= 2 INCHES : 30%
PROB OF 6 OR MORE COMBINED SEVERE HAIL/WIND EVENTS : 95%

MAX HAIL /INCHES/ : 2.0
MAX WIND GUSTS SURFACE /KNOTS/ : 70
MAX TOPS /X 100 FEET/ : 500
MEAN STORM MOTION VECTOR /DEGREES AND KNOTS/ : 24035
PARTICULARLY DANGEROUS SITUATION : NO";
        let bulletin = parse_wwp_bulletin(text).expect("wwp bulletin");
        assert_eq!(bulletin.watch_type, SpcWatchType::Tornado);
        assert_eq!(bulletin.watch_number, 31);
        assert_eq!(bulletin.prob_combined_hail_wind_6_or_more, 95);
    }

    #[test]
    fn parses_watch_probabilities_with_ws_header() {
        let text = "\
SEVERE THUNDERSTORM WATCH PROBABILITIES FOR WS 0071

PROB OF 2 OR MORE TORNADOES                        :  20%
PROB OF 1 OR MORE STRONG /EF2-EF5/ TORNADOES       :  05%
PROB OF 10 OR MORE SEVERE WIND EVENTS              :  50%
PROB OF 1 OR MORE WIND EVENTS >= 65 KNOTS          :  20%
PROB OF 10 OR MORE SEVERE HAIL EVENTS              : <05%
PROB OF 1 OR MORE HAIL EVENTS >= 2 INCHES          : <05%
PROB OF 6 OR MORE COMBINED SEVERE HAIL/WIND EVENTS :  70%

MAX HAIL /INCHES/                            : 0.0
MAX WIND GUSTS SURFACE /KNOTS/               : 60
MAX TOPS /X 100 FEET/                        : 200
MEAN STORM MOTION VECTOR /DEGREES AND KNOTS/ : 25040
PARTICULARLY DANGEROUS SITUATION             : NO";
        let bulletin = parse_wwp_bulletin(text).expect("wwp bulletin");

        assert_eq!(bulletin.watch_type, SpcWatchType::SevereThunderstorm);
        assert_eq!(bulletin.watch_number, 71);
        assert_eq!(bulletin.prob_severe_wind_10_or_more, 50);
    }
}
