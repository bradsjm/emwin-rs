//! Row-to-contract mapping owned by the Postgres adapter.

use emwin_service::{
    ArchivedIssue, ArchivedProductDetail, ArchivedProductSummary, CellAggregateBucket,
    FacetAggregateBucket, IncidentSummary, TimeseriesAggregateBucket,
};
use serde_json::Value;
use sqlx::Row;
use sqlx::postgres::PgRow;
use sqlx::types::Json as SqlxJson;

pub(crate) fn incident_summary_from_row(row: &PgRow) -> IncidentSummary {
    IncidentSummary {
        office: row.get("office"),
        phenomena: row.get("phenomena"),
        significance: row.get("significance"),
        etn: row.get("etn"),
        current_status: row.get("current_status"),
        latest_vtec_action: row.get("latest_vtec_action"),
        issued_at: row.get("issued_at"),
        start_utc: row.get("start_utc"),
        end_utc: row.get("end_utc"),
        last_updated_at: row.get("last_updated_at"),
        first_product_id: row.get("first_product_id"),
        latest_product_id: row.get("latest_product_id"),
        latest_product_timestamp_utc: row.get("latest_product_timestamp_utc"),
    }
}

pub(crate) fn archived_product_summary_from_row(row: &PgRow) -> ArchivedProductSummary {
    ArchivedProductSummary {
        product_id: row.get("product_id"),
        filename: row.get("filename"),
        source_timestamp_utc: row.get("source_timestamp_utc"),
        ingested_at: row.get("ingested_at"),
        source_receiver: row.get("source_receiver"),
        source_message_id: row.get("source_message_id"),
        size_bytes: row.get("size_bytes"),
        has_metadata_sidecar: row.get("has_metadata_sidecar"),
        source: row.get("source"),
        family: row.get("family"),
        artifact_kind: row.get("artifact_kind"),
        title: row.get("title"),
        container: row.get("container"),
        pil: row.get("pil"),
        wmo_prefix: row.get("wmo_prefix"),
        bbb_kind: row.get("bbb_kind"),
        office_code: row.get("office_code"),
        office_city: row.get("office_city"),
        office_state: row.get("office_state"),
        header_kind: row.get("header_kind"),
        ttaaii: row.get("ttaaii"),
        cccc: row.get("cccc"),
        ddhhmm: row.get("ddhhmm"),
        bbb: row.get("bbb"),
        afos: row.get("afos"),
        has_body: row.get("has_body"),
        has_artifact: row.get("has_artifact"),
        has_issues: row.get("has_issues"),
        has_vtec: row.get("has_vtec"),
        has_ugc: row.get("has_ugc"),
        has_hvtec: row.get("has_hvtec"),
        has_latlon: row.get("has_latlon"),
        has_time_mot_loc: row.get("has_time_mot_loc"),
        has_wind_hail: row.get("has_wind_hail"),
        vtec_count: row.get("vtec_count"),
        ugc_count: row.get("ugc_count"),
        hvtec_count: row.get("hvtec_count"),
        latlon_count: row.get("latlon_count"),
        time_mot_loc_count: row.get("time_mot_loc_count"),
        wind_hail_count: row.get("wind_hail_count"),
        issue_count: row.get("issue_count"),
    }
}

pub(crate) fn archived_product_detail_from_row(row: &PgRow) -> ArchivedProductDetail {
    ArchivedProductDetail {
        summary: archived_product_summary_from_row(row),
        payload_location: row.get("payload_location"),
        metadata_location: row.get("metadata_location"),
        product_json: row.get::<SqlxJson<Value>, _>("product_json").0,
    }
}

pub(crate) fn archived_issue_from_row(row: &PgRow) -> ArchivedIssue {
    ArchivedIssue {
        id: row.get("id"),
        product_id: row.get("product_id"),
        kind: row.get("kind"),
        code: row.get("code"),
        message: row.get("message"),
        line: row.get("line"),
    }
}

pub(crate) fn facet_aggregate_bucket_from_row(row: &PgRow) -> FacetAggregateBucket {
    FacetAggregateBucket {
        value: row.get("value"),
        count: row.get("count"),
    }
}

pub(crate) fn timeseries_aggregate_bucket_from_row(row: &PgRow) -> TimeseriesAggregateBucket {
    TimeseriesAggregateBucket {
        bucket_start: row.get("bucket_start"),
        bucket_end: row.get("bucket_end"),
        count: row.get("count"),
    }
}

pub(crate) fn cell_aggregate_bucket_from_row(row: &PgRow) -> CellAggregateBucket {
    CellAggregateBucket {
        cell: row.get("cell"),
        count: row.get("count"),
    }
}
