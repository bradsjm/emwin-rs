//! Shared state and response payloads for live server mode.
//!
//! Keeping these types in one place helps the HTTP layer, ingest loop, and retention code agree
//! on stable payload shapes without circular dependencies.

mod filters;
mod payloads;
mod query;
mod state;
mod urls;

pub(crate) use filters::{EventFilter, IncidentEventFilter};
#[cfg(test)]
pub(crate) use payloads::TelemetryPayload;
pub(crate) use payloads::{
    ArchiveIssuePayload, ArchiveIssueResponse, ArchiveIssuesResponse, ArchiveProductDetailPayload,
    ArchiveProductResponse, ArchiveProductSummaryPayload, ArchiveStatus, ArchivedFeaturePayload,
    CellAggregateResponse, CompletedFileEventPayload, CompletedFilePayload, EventKind,
    FacetAggregateResponse, FeatureCollectionResponse, FeaturesResponse, FilesResponse,
    HealthResponse, IncidentDetailPayload, IncidentEventPayload, IncidentProductsResponse,
    IncidentResponse, IncidentSummaryPayload, IncidentsResponse, MetricsPayload, ProductsResponse,
    TimeseriesAggregateResponse,
};
pub(crate) use query::{
    ArchiveFilterParams, ArchiveIssuesQuery, CellAggregateHttpQuery, EventsQuery,
    FacetAggregateHttpQuery, FeaturesGeoJsonQuery, FeaturesQuery, IncidentEventsQuery,
    IncidentProductsQuery, IncidentsQuery, ProductsQuery, TimeseriesAggregateHttpQuery,
};
pub use state::{ApiArchiveStatus, ApiServices, HttpServerOptions};
pub(crate) use state::{AppState, BroadcastEvent, ClientGuard, IncidentBroadcastEvent};
pub(crate) use urls::{API_PREFIX, OPENAPI_AUTH_SCHEME_NAME, OPENAPI_JSON_PATH};

#[cfg(test)]
mod tests;
