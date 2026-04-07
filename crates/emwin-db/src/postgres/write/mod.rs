//! Write-side Postgres persistence helpers.
//!
//! This module owns the product upsert flow, child row replacement, and
//! incident projection writes used by the metadata sink.

use super::prepare::PreparedProduct;
use super::{PersistResult, Postgres};
use sqlx::Transaction;

mod children;
mod incidents;
mod product;

pub(super) async fn upsert_product(
    tx: &mut Transaction<'_, Postgres>,
    prepared: &PreparedProduct,
) -> PersistResult<i64> {
    product::upsert_product(tx, prepared).await
}

pub(super) async fn replace_children(
    tx: &mut Transaction<'_, Postgres>,
    product_id: i64,
    prepared: &PreparedProduct,
) -> PersistResult<Vec<super::prepare::PendingIncidentChange>> {
    children::replace_children(tx, product_id, prepared).await
}
