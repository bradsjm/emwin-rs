use super::models::{
    ArchivedFeature, ArchivedIssue, ArchivedPayload, ArchivedProductDetail, ArchivedProductSummary,
    CellAggregateResult, FacetAggregateResult, IncidentDetail, IncidentSummary, PaginatedResponse,
    TimeseriesAggregateResult,
};
use super::query::{
    ArchivedIssueListQuery, CellAggregateQuery, FacetAggregateQuery, FeatureListQuery, IncidentKey,
    IncidentListQuery, IncidentProductsQuery, ProductListQuery, TimeseriesAggregateQuery,
};
use crate::error::ServiceResult;
use std::future::Future;
use std::pin::Pin;
use tokio::sync::broadcast;

/// Boxed future type used by service traits to avoid forcing async-trait.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Stable archive query interface implemented by storage adapters.
pub trait ArchiveQueryService: Send + Sync {
    fn list_incidents(
        &self,
        query: IncidentListQuery,
    ) -> BoxFuture<'_, ServiceResult<PaginatedResponse<IncidentSummary>>>;
    fn get_incident<'a>(
        &'a self,
        key: &'a IncidentKey,
    ) -> BoxFuture<'a, ServiceResult<Option<IncidentDetail>>>;
    fn list_incident_products<'a>(
        &'a self,
        key: &'a IncidentKey,
        query: IncidentProductsQuery,
    ) -> BoxFuture<'a, ServiceResult<PaginatedResponse<ArchivedProductSummary>>>;
    fn list_archived_products(
        &self,
        query: ProductListQuery,
    ) -> BoxFuture<'_, ServiceResult<PaginatedResponse<ArchivedProductSummary>>>;
    fn get_archived_product(
        &self,
        product_id: i64,
    ) -> BoxFuture<'_, ServiceResult<Option<ArchivedProductDetail>>>;
    fn list_archived_issues(
        &self,
        query: ArchivedIssueListQuery,
    ) -> BoxFuture<'_, ServiceResult<PaginatedResponse<ArchivedIssue>>>;
    fn get_archived_issue(
        &self,
        issue_id: i64,
    ) -> BoxFuture<'_, ServiceResult<Option<ArchivedIssue>>>;
    fn read_archived_payload(
        &self,
        product_id: i64,
    ) -> BoxFuture<'_, ServiceResult<Option<ArchivedPayload>>>;
    fn list_archived_features(
        &self,
        query: FeatureListQuery,
    ) -> BoxFuture<'_, ServiceResult<PaginatedResponse<ArchivedFeature>>>;
    fn list_facet_aggregate(
        &self,
        query: FacetAggregateQuery,
    ) -> BoxFuture<'_, ServiceResult<FacetAggregateResult>>;
    fn list_timeseries_aggregate(
        &self,
        query: TimeseriesAggregateQuery,
    ) -> BoxFuture<'_, ServiceResult<TimeseriesAggregateResult>>;
    fn list_cell_aggregate(
        &self,
        query: CellAggregateQuery,
    ) -> BoxFuture<'_, ServiceResult<CellAggregateResult>>;
}

/// Optional incident-change stream exposed by archive implementations with projection updates.
pub trait IncidentChangeStream: Send + Sync {
    fn subscribe_incident_changes(
        &self,
    ) -> Option<broadcast::Receiver<crate::live::IncidentBroadcastEvent>>;
}
