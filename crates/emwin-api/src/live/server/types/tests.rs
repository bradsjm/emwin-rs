use super::{
    ArchiveFilterParams, CompletedFileEventPayload, EventFilter, EventKind, EventsQuery,
    TelemetryPayload,
};
use crate::live::server::server_http::event_matches_filter;
use emwin_db::CompletedFileMetadata;
use emwin_parser::{detail_product_v2, enrich_product, summarize_product_v2};
use emwin_protocol::ingest::ProductOrigin;

fn empty_events_query() -> EventsQuery {
    EventsQuery {
        event: None,
        filename: None,
        source: None,
        pil: None,
        family: None,
        container: None,
        wmo_prefix: None,
        office: None,
        office_city: None,
        office_state: None,
        bbb_kind: None,
        cccc: None,
        ttaaii: None,
        afos: None,
        bbb: None,
        has_issues: None,
        issue_kind: None,
        issue_code: None,
        has_vtec: None,
        has_ugc: None,
        has_hvtec: None,
        has_latlon: None,
        has_time_mot_loc: None,
        has_wind_hail: None,
        state: None,
        county: None,
        zone: None,
        fire_zone: None,
        marine_zone: None,
        vtec_phenomena: None,
        vtec_significance: None,
        vtec_action: None,
        vtec_office: None,
        etn: None,
        hvtec_nwslid: None,
        hvtec_severity: None,
        hvtec_cause: None,
        hvtec_record: None,
        wind_hail_kind: None,
        lat: None,
        lon: None,
        distance_miles: None,
        min_lat: None,
        max_lat: None,
        min_lon: None,
        max_lon: None,
        min_wind_mph: None,
        min_hail_inches: None,
        min_size: None,
        max_size: None,
    }
}

fn file_complete_event(filename: &str) -> EventKind {
    let data = if filename.eq_ignore_ascii_case("TAFPDKGA.TXT") {
        b"000 \nFTUS42 KFFC 022320\nTAFPDK\nBody\n".as_slice()
    } else if filename.eq_ignore_ascii_case("TAFWBCFJ.TXT") {
        b"000 \nFTXX01 KWBC 070200\nTAF AMD\nWBCF 070244Z 0703/0803 18012KT P6SM SCT050\n"
            .as_slice()
    } else if filename.eq_ignore_ascii_case("TAFWMOONLY.TXT") {
        b"000 \nFTUS80 KWBC 070200\nTAF SBAF 070200Z 0703/0803 00000KT CAVOK=\n".as_slice()
    } else if filename.eq_ignore_ascii_case("BROKEN.TXT") {
        b"000 \nINVALID HEADER\nAFDBOX\nBody\n".as_slice()
    } else if filename.eq_ignore_ascii_case("SVROAXNE.TXT") {
        br#"000
WUUS53 KOAX 051200
SVROAX

URGENT - IMMEDIATE BROADCAST REQUESTED
Severe Thunderstorm Warning
National Weather Service Omaha/Valley NE
1200 PM CST Wed Mar 5 2025

NEC001>003-051300-
/O.NEW.KOAX.SV.W.0001.250305T1200Z-250305T1800Z/

Severe Thunderstorm Warning for...
East Central Cuming County in northeastern Nebraska...

This is a test product.
$$
"#
        .as_slice()
    } else if filename.eq_ignore_ascii_case("SVRWIND.TXT") {
        br#"000
WUUS53 KOAX 051200
SVROAX

URGENT - IMMEDIATE BROADCAST REQUESTED
Severe Thunderstorm Warning
National Weather Service Omaha/Valley NE
1200 PM CST Wed Mar 5 2025

NEC001>003-051300-
/O.NEW.KOAX.SV.W.0001.250305T1200Z-250305T1800Z/

LAT...LON 4143 9613 4145 9610 4140 9608 4138 9612
TIME...MOT...LOC 1200Z 300DEG 25KT 4143 9613 4140 9608
HAILTHREAT...RADARINDICATED
MAXHAILSIZE...1.00 IN
WINDTHREAT...OBSERVED
MAXWINDGUST...60 MPH
"#
        .as_slice()
    } else if filename.eq_ignore_ascii_case("SVRPOLY.TXT") {
        br#"000
WUUS53 KOAX 051200
SVROAX

URGENT - IMMEDIATE BROADCAST REQUESTED
Severe Thunderstorm Warning
National Weather Service Omaha/Valley NE
1200 PM CST Wed Mar 5 2025

NEC001>003-051300-
/O.NEW.KOAX.SV.W.0001.250305T1200Z-250305T1800Z/

LAT...LON 4143 9613 4145 9610 4140 9608 4138 9612
"#
        .as_slice()
    } else if filename.eq_ignore_ascii_case("SVRALC.TXT") {
        br#"000
WUUS54 KBMX 051200
SVRBMX

URGENT - IMMEDIATE BROADCAST REQUESTED
Severe Thunderstorm Warning
National Weather Service Birmingham AL
1200 PM CST Wed Mar 5 2025

ALC001-051300-
/O.NEW.KBMX.SV.W.0001.250305T1200Z-250305T1800Z/
"#
        .as_slice()
    } else if filename.eq_ignore_ascii_case("FFWOAXNE.TXT") {
        br#"000
WUUS53 KOAX 051200
FFWOAX

Flash Flood Warning
National Weather Service Omaha/Valley NE
1200 PM CST Wed Mar 5 2025

NEC001>003-051300-
/O.NEW.KOAX.FF.W.0001.250305T1200Z-250305T1800Z/
/MSRM1.3.ER.250305T1200Z.250305T1800Z.250306T0000Z.NO/

LAT...LON 4143 9613 4145 9610 4140 9608 4138 9612
TIME...MOT...LOC 1200Z 300DEG 25KT 4143 9613 4140 9608
"#
        .as_slice()
    } else if filename.eq_ignore_ascii_case("FFWCHFA2.TXT") {
        br#"000
WGUS53 PAFG 051200
FFWAFG

Flash Flood Warning
National Weather Service Fairbanks AK
1200 PM AKST Wed Mar 5 2025

AKC090-051300-
/O.NEW.PAFG.FF.W.0001.250305T1200Z-250305T1800Z/
/CHFA2.3.ER.250305T1200Z.250305T1800Z.250306T0000Z.NO/
"#
        .as_slice()
    } else {
        b"ignored".as_slice()
    };

    let product = enrich_product(filename, data);
    let metadata = CompletedFileMetadata {
        filename: filename.to_string(),
        size: 11,
        timestamp_utc: 1,
        origin: ProductOrigin::Qbt,
        product_summary: summarize_product_v2(&product),
        product_detail: detail_product_v2(&product),
        product,
    };

    EventKind::FileComplete(Box::new(CompletedFileEventPayload::from_metadata(metadata)))
}

fn event_filter(query: EventsQuery) -> EventFilter {
    EventFilter::from_query(query)
}

#[test]
fn events_filter_only_allows_matching_filenames() {
    let txt = file_complete_event("report.txt");
    let zip = file_complete_event("report.zip");
    let telemetry = EventKind::Telemetry(TelemetryPayload::Unavailable);
    let filter = EventFilter::from_query(EventsQuery {
        filename: Some("*.txt".to_string()),
        ..empty_events_query()
    });

    assert!(event_matches_filter(&filter, &txt));
    assert!(!event_matches_filter(&filter, &zip));
    assert!(!event_matches_filter(&filter, &telemetry));
}

#[test]
fn events_filter_matches_structured_metadata_fields() {
    let filter = EventFilter::from_query(EventsQuery {
        event: Some("product_available".to_string()),
        pil: Some("taf,afd".to_string()),
        office: Some("ffc".to_string()),
        office_state: Some("ga".to_string()),
        cccc: Some("kffc".to_string()),
        family: Some("NWS_TEXT_PRODUCT".to_string()),
        container: Some("raw".to_string()),
        ..empty_events_query()
    });
    let event = file_complete_event("TAFPDKGA.TXT");

    assert!(event_matches_filter(&filter, &event));
}

#[test]
fn events_filter_matches_office_city_for_wmo_only_fallbacks() {
    let event = file_complete_event("TAFWBCFJ.TXT");
    let filter = EventFilter::from_query(EventsQuery {
        office: Some("wbc".to_string()),
        office_city: Some("national centers for environmental prediction".to_string()),
        office_state: Some("md".to_string()),
        ..empty_events_query()
    });

    assert!(event_matches_filter(&filter, &event));
}

#[test]
fn events_filter_matches_wmo_header_fallback_fields() {
    let event = file_complete_event("TAFWMOONLY.TXT");
    let filter = EventFilter::from_query(EventsQuery {
        source: Some("wmo_bulletin".to_string()),
        cccc: Some("kwbc".to_string()),
        ttaaii: Some("ftus80".to_string()),
        ..empty_events_query()
    });

    assert!(event_matches_filter(&filter, &event));
}

#[test]
fn events_filter_matches_geographic_codes() {
    let event = file_complete_event("SVROAXNE.TXT");
    let filter = EventFilter::from_query(EventsQuery {
        state: Some("ne".to_string()),
        county: Some("NEC002".to_string()),
        ..empty_events_query()
    });

    assert!(event_matches_filter(&filter, &event));
}

#[test]
fn events_filter_requires_matching_geographic_class() {
    let event = file_complete_event("SVROAXNE.TXT");
    let filter = EventFilter::from_query(EventsQuery {
        zone: Some("NEZ002".to_string()),
        ..empty_events_query()
    });

    assert!(!event_matches_filter(&filter, &event));
}

#[test]
fn events_filter_matches_vtec_codes() {
    let event = file_complete_event("SVROAXNE.TXT");
    let filter = EventFilter::from_query(EventsQuery {
        vtec_phenomena: Some("sv".to_string()),
        vtec_significance: Some("w".to_string()),
        vtec_action: Some("new".to_string()),
        vtec_office: Some("koax".to_string()),
        etn: Some("1,99".to_string()),
        ..empty_events_query()
    });

    assert!(event_matches_filter(&filter, &event));
}

#[test]
fn events_filter_rejects_non_matching_vtec_codes() {
    let event = file_complete_event("SVROAXNE.TXT");
    let filter = EventFilter::from_query(EventsQuery {
        vtec_action: Some("CAN".to_string()),
        ..empty_events_query()
    });

    assert!(!event_matches_filter(&filter, &event));
}

#[test]
fn events_filter_matches_issue_fields() {
    let event = file_complete_event("BROKEN.TXT");
    let filter = EventFilter::from_query(EventsQuery {
        has_issues: Some("true".to_string()),
        issue_kind: Some("text_product_parse".to_string()),
        issue_code: Some("invalid_wmo_header".to_string()),
        ..empty_events_query()
    });

    assert!(event_matches_filter(&filter, &event));
}

#[test]
fn archive_filter_params_reject_invalid_boolean_literals() {
    let error = ArchiveFilterParams {
        has_issues: Some("maybe".to_string()),
        ..ArchiveFilterParams::default()
    }
    .into_product_list_query(100, None, None)
    .expect_err("invalid boolean literal should fail");

    assert!(error.contains("has_issues must be one of"));
}

#[test]
fn archive_filter_params_preserve_artifact_kind() {
    let query = ArchiveFilterParams {
        artifact_kind: Some("nws_text_product,cap_message".to_string()),
        ..ArchiveFilterParams::default()
    }
    .into_product_list_query(100, None, None)
    .expect("query should build");

    assert_eq!(
        query.artifact_kind.as_deref(),
        Some("nws_text_product,cap_message")
    );
}

#[test]
fn events_filter_matches_body_presence_for_hvtec_product() {
    let event = file_complete_event("FFWOAXNE.TXT");
    let filter = EventFilter::from_query(EventsQuery {
        has_vtec: Some("true".to_string()),
        has_ugc: Some("true".to_string()),
        has_hvtec: Some("true".to_string()),
        has_latlon: Some("true".to_string()),
        has_time_mot_loc: Some("true".to_string()),
        ..empty_events_query()
    });

    assert!(event_matches_filter(&filter, &event));
}

#[test]
fn events_filter_matches_body_presence_for_wind_hail_product() {
    let event = file_complete_event("SVRWIND.TXT");
    let filter = EventFilter::from_query(EventsQuery {
        has_vtec: Some("true".to_string()),
        has_ugc: Some("true".to_string()),
        has_latlon: Some("true".to_string()),
        has_time_mot_loc: Some("true".to_string()),
        has_wind_hail: Some("true".to_string()),
        ..empty_events_query()
    });

    assert!(event_matches_filter(&filter, &event));
}

#[test]
fn events_filter_matches_hvtec_fields() {
    let event = file_complete_event("FFWOAXNE.TXT");
    let filter = EventFilter::from_query(EventsQuery {
        hvtec_nwslid: Some("MSRM1".to_string()),
        hvtec_severity: Some("major".to_string()),
        hvtec_cause: Some("excessive_rainfall".to_string()),
        hvtec_record: Some("no_record".to_string()),
        ..empty_events_query()
    });

    assert!(event_matches_filter(&filter, &event));
}

#[test]
fn events_filter_matches_wind_hail_fields() {
    let event = file_complete_event("SVRWIND.TXT");
    let filter = EventFilter::from_query(EventsQuery {
        wind_hail_kind: Some("hail_threat,max_wind_gust".to_string()),
        min_wind_mph: Some(50.0),
        min_hail_inches: Some(1.0),
        ..empty_events_query()
    });

    assert!(event_matches_filter(&filter, &event));
}

#[test]
fn events_filter_requires_matching_header_metadata() {
    let filter = EventFilter::from_query(EventsQuery {
        ttaaii: Some("WWUS53".to_string()),
        ..empty_events_query()
    });
    let event = file_complete_event("TAFPDKGA.TXT");

    assert!(!event_matches_filter(&filter, &event));
}

#[test]
fn events_filter_matches_non_file_events_only_by_event_name() {
    let telemetry = EventKind::Telemetry(TelemetryPayload::Unavailable);
    let filter = EventFilter::from_query(EventsQuery {
        event: Some("telemetry,connected".to_string()),
        ..empty_events_query()
    });

    assert!(event_matches_filter(&filter, &telemetry));
}

#[test]
fn events_filter_matches_polygon_containment_without_point_distance() {
    let event = file_complete_event("SVRPOLY.TXT");
    let filter = event_filter(EventsQuery {
        lat: Some(41.43),
        lon: Some(-96.13),
        ..empty_events_query()
    });

    assert!(event_matches_filter(&filter, &event));
}

#[test]
fn events_filter_matches_time_mot_loc_points_within_default_radius() {
    let event = file_complete_event("SVRWIND.TXT");
    let filter = event_filter(EventsQuery {
        lat: Some(41.43),
        lon: Some(-96.13),
        ..empty_events_query()
    });

    assert!(event_matches_filter(&filter, &event));
}

#[test]
fn events_filter_matches_ugc_representative_points_within_radius() {
    let event = file_complete_event("SVRALC.TXT");
    let filter = event_filter(EventsQuery {
        lat: Some(32.5349),
        lon: Some(-86.6428),
        distance_miles: Some(1.0),
        ..empty_events_query()
    });

    assert!(event_matches_filter(&filter, &event));
}

#[test]
fn events_filter_matches_hvtec_gauge_points_within_radius() {
    let event = file_complete_event("FFWCHFA2.TXT");
    let filter = event_filter(EventsQuery {
        lat: Some(64.8458),
        lon: Some(-147.7011),
        distance_miles: Some(1.0),
        ..empty_events_query()
    });

    assert!(event_matches_filter(&filter, &event));
}

#[test]
fn events_filter_rejects_products_without_spatial_data() {
    let event = file_complete_event("TAFPDKGA.TXT");
    let filter = event_filter(EventsQuery {
        lat: Some(41.42),
        lon: Some(-96.17),
        ..empty_events_query()
    });

    assert!(!event_matches_filter(&filter, &event));
}

#[test]
fn events_filter_does_not_match_non_file_events_with_location_constraints() {
    let telemetry = EventKind::Telemetry(TelemetryPayload::Unavailable);
    let filter = event_filter(EventsQuery {
        event: Some("telemetry".to_string()),
        lat: Some(41.42),
        lon: Some(-96.17),
        ..empty_events_query()
    });

    assert!(!event_matches_filter(&filter, &telemetry));
}

#[test]
fn events_filter_matches_bbox_against_polygon_and_points() {
    let polygon_event = file_complete_event("SVRPOLY.TXT");
    let path_event = file_complete_event("SVRWIND.TXT");
    let filter = event_filter(EventsQuery {
        min_lat: Some(41.39),
        max_lat: Some(41.46),
        min_lon: Some(-96.14),
        max_lon: Some(-96.07),
        ..empty_events_query()
    });

    assert!(event_matches_filter(&filter, &polygon_event));
    assert!(event_matches_filter(&filter, &path_event));
}

#[test]
fn events_filter_rejects_products_outside_bbox() {
    let event = file_complete_event("SVROAXNE.TXT");
    let filter = event_filter(EventsQuery {
        min_lat: Some(35.0),
        max_lat: Some(35.1),
        min_lon: Some(-82.1),
        max_lon: Some(-82.0),
        ..empty_events_query()
    });

    assert!(!event_matches_filter(&filter, &event));
}

#[test]
fn file_complete_event_includes_download_url() {
    let value = file_complete_event("nested/my file.txt").to_json();

    assert_eq!(value["download_url"], "/v1/files/nested%2Fmy%20file.txt");
    assert_eq!(value["timestamp_utc"], 1);
    assert_eq!(value["product"]["schema_version"], 2);
    assert!(value["product"].get("facets").is_some());
    assert!(value["product"].get("issues").is_some());
    assert!(value["product"].get("artifact").is_none());
}
