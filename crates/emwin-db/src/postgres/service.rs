use super::PostgresMetadataSink;
use crate::error::PersistError;
use crate::writer::BoxFuture;
use emwin_service::{
    ArchiveQueryService, ArchivedFeature, ArchivedIssue, ArchivedIssueListQuery, ArchivedPayload,
    ArchivedProductDetail, ArchivedProductSummary, CellAggregateQuery, CellAggregateResult,
    FacetAggregateQuery, FacetAggregateResult, FeatureListQuery, IncidentDetail, IncidentKey,
    IncidentListQuery, IncidentProductsQuery, IncidentSummary, PaginatedResponse,
    ProductListQuery, ServiceError, ServiceResult, TimeseriesAggregateQuery,
    TimeseriesAggregateResult,
};

fn map_service_error(err: PersistError) -> ServiceError {
    match err {
        PersistError::InvalidRequest(message) => ServiceError::InvalidRequest(message),
        PersistError::InvalidConfig(message) => ServiceError::InvalidConfig(message),
        PersistError::Io(io) => ServiceError::Io(io),
        other => ServiceError::Runtime(other.to_string()),
    }
}

impl ArchiveQueryService for PostgresMetadataSink {
    fn list_incidents(
        &self,
        query: IncidentListQuery,
    ) -> BoxFuture<'_, ServiceResult<PaginatedResponse<IncidentSummary>>> {
        Box::pin(async move {
            PostgresMetadataSink::list_incidents(self, query)
                .await
                .map_err(map_service_error)
        })
    }

    fn get_incident<'a>(
        &'a self,
        key: &'a IncidentKey,
    ) -> BoxFuture<'a, ServiceResult<Option<IncidentDetail>>> {
        Box::pin(async move {
            PostgresMetadataSink::get_incident(self, key)
                .await
                .map_err(map_service_error)
        })
    }

    fn list_incident_products<'a>(
        &'a self,
        key: &'a IncidentKey,
        query: IncidentProductsQuery,
    ) -> BoxFuture<'a, ServiceResult<PaginatedResponse<ArchivedProductSummary>>> {
        Box::pin(async move {
            PostgresMetadataSink::list_incident_products(self, key, query)
                .await
                .map_err(map_service_error)
        })
    }

    fn list_archived_products(
        &self,
        query: ProductListQuery,
    ) -> BoxFuture<'_, ServiceResult<PaginatedResponse<ArchivedProductSummary>>> {
        Box::pin(async move {
            PostgresMetadataSink::list_archived_products(self, query)
                .await
                .map_err(map_service_error)
        })
    }

    fn get_archived_product(
        &self,
        product_id: i64,
    ) -> BoxFuture<'_, ServiceResult<Option<ArchivedProductDetail>>> {
        Box::pin(async move {
            PostgresMetadataSink::get_archived_product(self, product_id)
                .await
                .map_err(map_service_error)
        })
    }

    fn list_archived_issues(
        &self,
        query: ArchivedIssueListQuery,
    ) -> BoxFuture<'_, ServiceResult<PaginatedResponse<ArchivedIssue>>> {
        Box::pin(async move {
            PostgresMetadataSink::list_archived_issues(self, query)
                .await
                .map_err(map_service_error)
        })
    }

    fn get_archived_issue(
        &self,
        issue_id: i64,
    ) -> BoxFuture<'_, ServiceResult<Option<ArchivedIssue>>> {
        Box::pin(async move {
            PostgresMetadataSink::get_archived_issue(self, issue_id)
                .await
                .map_err(map_service_error)
        })
    }

    fn read_archived_payload(
        &self,
        product_id: i64,
    ) -> BoxFuture<'_, ServiceResult<Option<ArchivedPayload>>> {
        Box::pin(async move {
            PostgresMetadataSink::read_archived_payload(self, product_id)
                .await
                .map_err(map_service_error)
        })
    }

    fn list_archived_features(
        &self,
        query: FeatureListQuery,
    ) -> BoxFuture<'_, ServiceResult<PaginatedResponse<ArchivedFeature>>> {
        Box::pin(async move {
            PostgresMetadataSink::list_archived_features(self, query)
                .await
                .map_err(map_service_error)
        })
    }

    fn list_facet_aggregate(
        &self,
        query: FacetAggregateQuery,
    ) -> BoxFuture<'_, ServiceResult<FacetAggregateResult>> {
        Box::pin(async move {
            PostgresMetadataSink::list_facet_aggregate(self, query)
                .await
                .map_err(map_service_error)
        })
    }

    fn list_timeseries_aggregate(
        &self,
        query: TimeseriesAggregateQuery,
    ) -> BoxFuture<'_, ServiceResult<TimeseriesAggregateResult>> {
        Box::pin(async move {
            PostgresMetadataSink::list_timeseries_aggregate(self, query)
                .await
                .map_err(map_service_error)
        })
    }

    fn list_cell_aggregate(
        &self,
        query: CellAggregateQuery,
    ) -> BoxFuture<'_, ServiceResult<CellAggregateResult>> {
        Box::pin(async move {
            PostgresMetadataSink::list_cell_aggregate(self, query)
                .await
                .map_err(map_service_error)
        })
    }
}
