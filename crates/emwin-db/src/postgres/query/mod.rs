//! Postgres archive and incident query helpers.
//!
//! This module wires the read-path submodules together and keeps only the
//! small shared helpers needed across query families.

use super::{
    IncidentChange, IncidentChangeTrigger, IncidentCursor, IncidentKey, IncidentListQuery,
    IncidentProductsCursor, IncidentProductsQuery, IncidentSummary, PaginatedResponse,
    PersistError, PersistResult, ProductListQuery,
};
use emwin_service::FacetDimension;

mod archive;
mod filters;
mod incidents;
mod mappers;
mod spatial;
mod sql;
mod validation;

pub(super) use archive::{
    get_archived_issue_query, list_archived_features_query, list_archived_issues_query,
    list_archived_products_query, list_cell_aggregate_query, list_facet_aggregate_query,
    list_timeseries_aggregate_query,
};
pub(super) use incidents::{
    list_incident_products_query, list_incidents_query, load_incident_changes,
};
pub(super) use mappers::{archived_product_detail_from_row, incident_summary_from_row};
pub(super) use sql::{
    archived_feature_select_sql, archived_issue_select_sql, archived_product_detail_select_sql,
    archived_product_summary_select_sql, incident_select_sql,
};
pub(super) use validation::{
    decode_optional_cursor, encode_cursor, normalize_lower, normalize_upper, split_csv_i64,
    split_csv_values,
};

pub(super) fn normalize_page_limit(limit: usize) -> usize {
    limit.clamp(1, 500)
}
