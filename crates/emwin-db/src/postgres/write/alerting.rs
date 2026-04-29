use super::super::prepare::PendingIncidentChange;
use super::super::{PersistError, PersistResult, Postgres, QueryBuilder, query};
use crate::metadata::CompletedFileMetadata;
use chrono::TimeZone;
use emwin_service::{AlertSourceKind, IncidentChangeTrigger};
use sqlx::Transaction;

pub(super) async fn insert_product_source_event(
    tx: &mut Transaction<'_, Postgres>,
    product_id: i64,
    metadata: &CompletedFileMetadata,
) -> PersistResult<()> {
    let source_timestamp = chrono::Utc
        .timestamp_opt(
            i64::try_from(metadata.timestamp_utc).map_err(|_| {
                PersistError::InvalidRequest(format!(
                    "invalid product source timestamp `{}`",
                    metadata.timestamp_utc
                ))
            })?,
            0,
        )
        .single()
        .ok_or_else(|| {
            PersistError::InvalidRequest(format!(
                "invalid product source timestamp `{}`",
                metadata.timestamp_utc
            ))
        })?;

    sqlx::query(
        "INSERT INTO alerting.source_events (
            source_kind,
            source_id,
            payload_json,
            source_timestamp
        ) VALUES ($1, $2, $3, $4)",
    )
    .bind(serde_label(AlertSourceKind::ProductAvailable)?)
    .bind(product_id.to_string())
    .bind(serde_json::json!({
        "filename": metadata.filename,
        "size": metadata.size,
        "timestamp_utc": metadata.timestamp_utc,
        "origin": metadata.origin,
        "product": metadata.product,
        "product_summary": metadata.product_summary(),
        "product_detail": metadata.product_detail(),
    }))
    .bind(source_timestamp)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

pub(super) async fn insert_incident_source_events(
    tx: &mut Transaction<'_, Postgres>,
    changes: &[PendingIncidentChange],
    trigger: IncidentChangeTrigger,
) -> PersistResult<()> {
    for change in changes {
        let mut builder = QueryBuilder::<Postgres>::new(query::incident_select_sql());
        builder
            .push(" WHERE office = ")
            .push_bind(&change.key.office)
            .push(" AND phenomena = ")
            .push_bind(&change.key.phenomena)
            .push(" AND significance = ")
            .push_bind(&change.key.significance)
            .push(" AND etn = ")
            .push_bind(change.key.etn);
        let row = builder.build().fetch_one(&mut **tx).await?;
        let incident = query::incident_summary_from_row(&row);
        let payload = serde_json::to_value(emwin_service::IncidentChange {
            action: change.action,
            trigger,
            incident: incident.clone(),
        })
        .map_err(|err| PersistError::InvalidRequest(err.to_string()))?;

        sqlx::query(
            "INSERT INTO alerting.source_events (
                source_kind,
                source_id,
                payload_json,
                source_timestamp
            ) VALUES ($1, $2, $3, $4)",
        )
        .bind(serde_label(AlertSourceKind::IncidentChange)?)
        .bind(format!(
            "{}/{}/{}/{}:{}",
            change.key.office,
            change.key.phenomena,
            change.key.significance,
            change.key.etn,
            serde_label(change.action)?,
        ))
        .bind(payload)
        .bind(incident.last_updated_at)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

fn serde_label<T: serde::Serialize>(value: T) -> PersistResult<String> {
    serde_json::to_value(value)
        .map_err(|err| PersistError::InvalidRequest(err.to_string()))?
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| {
            PersistError::InvalidRequest("serde value did not serialize to string".into())
        })
}
