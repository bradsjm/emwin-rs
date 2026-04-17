//! Shared archive query contracts and helpers.
//!
//! This module defines the stable service-facing query/input/output types used across adapters.
//! Storage-specific row mapping and parser-backed metadata construction belong in implementation
//! crates such as `emwin-db` and `emwin-live`.

mod models;
mod query;
mod service;

pub use models::{
    AggregateCompleteness, ArchivedFeature, ArchivedIssue, ArchivedPayload, ArchivedProductDetail,
    ArchivedProductSummary, CellAggregateBucket, CellAggregateResult, FacetAggregateBucket,
    FacetAggregateResult, IncidentChange, IncidentChangeAction, IncidentChangeTrigger,
    IncidentCleanupResult, IncidentDetail, IncidentSummary, PaginatedResponse,
    TimeseriesAggregateBucket, TimeseriesAggregateResult,
};
pub use query::{
    ArchiveFilterInput, ArchivedIssueCursor, ArchivedIssueListQuery, CellAggregateQuery,
    CellMeasure, FacetAggregateQuery, FacetDimension, FeatureCursor, FeatureKind, FeatureListQuery,
    IncidentCursor, IncidentKey, IncidentListQuery, IncidentProductsCursor, IncidentProductsQuery,
    ProductCursor, ProductListQuery, TimeseriesAggregateQuery, TimeseriesBucket, TimeseriesMeasure,
    build_cell_aggregate_query, build_facet_aggregate_query, build_feature_list_query,
    build_timeseries_aggregate_query, decode_optional_cursor, encode_cursor, parse_archive_bool,
};
pub use service::{ArchiveQueryService, BoxFuture};
