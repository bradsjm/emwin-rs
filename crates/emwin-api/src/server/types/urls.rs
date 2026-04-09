use emwin_service::IncidentSummary;

pub(crate) const API_PREFIX: &str = "/v1";
pub(crate) const OPENAPI_JSON_PATH: &str = "/openapi.json";
pub(crate) const OPENAPI_AUTH_SCHEME_NAME: &str = "bearer_auth";

pub(crate) fn incident_detail_url(incident: &IncidentSummary) -> String {
    format!(
        "{API_PREFIX}/incidents/{}/{}/{}/{}",
        incident.office, incident.phenomena, incident.significance, incident.etn
    )
}

pub(crate) fn incident_products_url(incident: &IncidentSummary) -> String {
    format!(
        "{API_PREFIX}/incidents/{}/{}/{}/{}/products",
        incident.office, incident.phenomena, incident.significance, incident.etn
    )
}

pub(crate) fn archive_product_url(product_id: i64) -> String {
    format!("{API_PREFIX}/products/{product_id}")
}

pub(crate) fn archive_product_raw_url(product_id: i64) -> String {
    format!("{API_PREFIX}/products/{product_id}/raw")
}

pub(crate) fn archive_issue_url(issue_id: i64) -> String {
    format!("{API_PREFIX}/issues/{issue_id}")
}
