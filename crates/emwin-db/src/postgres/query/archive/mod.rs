//! Archive query entrypoints grouped by query family.
//!
//! Each submodule owns one archive concern so product, feature, aggregate,
//! and issue logic can evolve without reintroducing a single mixed query file.

mod aggregates;
mod features;
mod issues;
mod products;

pub(crate) use aggregates::{
    list_cell_aggregate_query, list_facet_aggregate_query, list_timeseries_aggregate_query,
};
pub(crate) use features::list_archived_features_query;
pub(crate) use issues::{get_archived_issue_query, list_archived_issues_query};
pub(crate) use products::{
    archived_product_detail_from_row, list_archived_products_query, parse_blob_storage_kind,
};
