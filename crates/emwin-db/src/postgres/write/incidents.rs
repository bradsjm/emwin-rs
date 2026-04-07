use super::super::prepare::PendingIncidentChange;
use super::super::{IncidentChangeAction, PersistResult, Postgres};
use sqlx::Transaction;

pub(super) async fn upsert_incidents(
    tx: &mut Transaction<'_, Postgres>,
    product_id: i64,
    rows: &[super::super::prepare::PreparedIncidentUpdate],
) -> PersistResult<Vec<PendingIncidentChange>> {
    let mut changes = Vec::with_capacity(rows.len());
    for row in rows {
        let existed = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                SELECT 1
                FROM incidents
                WHERE office = $1 AND phenomena = $2 AND significance = $3 AND etn = $4
            )",
        )
        .bind(&row.key.office)
        .bind(&row.key.phenomena)
        .bind(&row.key.significance)
        .bind(row.key.etn)
        .fetch_one(&mut **tx)
        .await?;

        let done = sqlx::query(
            "WITH existing AS (
                SELECT current_status
                FROM incidents
                WHERE office = $1 AND phenomena = $2 AND significance = $3 AND etn = $4
            )
            INSERT INTO incidents (
                office,
                phenomena,
                significance,
                etn,
                current_status,
                latest_vtec_action,
                issued_at,
                start_utc,
                end_utc,
                first_product_id,
                latest_product_id,
                latest_product_timestamp_utc
            )
            SELECT
                $1,
                $2,
                $3,
                $4,
                COALESCE($5, existing.current_status),
                $6,
                $7,
                $8,
                $9,
                $10,
                $11,
                $12
            FROM (SELECT 1) seed
            LEFT JOIN existing ON TRUE
            WHERE $5 IS NOT NULL OR existing.current_status IS NOT NULL
            ON CONFLICT (office, phenomena, significance, etn) DO UPDATE SET
                current_status = COALESCE(EXCLUDED.current_status, incidents.current_status),
                latest_vtec_action = EXCLUDED.latest_vtec_action,
                issued_at = EXCLUDED.issued_at,
                start_utc = EXCLUDED.start_utc,
                end_utc = EXCLUDED.end_utc,
                last_updated_at = now(),
                first_product_id = incidents.first_product_id,
                latest_product_id = EXCLUDED.latest_product_id,
                latest_product_timestamp_utc = EXCLUDED.latest_product_timestamp_utc
            WHERE EXCLUDED.latest_product_timestamp_utc >= incidents.latest_product_timestamp_utc",
        )
        .bind(&row.key.office)
        .bind(&row.key.phenomena)
        .bind(&row.key.significance)
        .bind(row.key.etn)
        .bind(&row.current_status)
        .bind(&row.latest_vtec_action)
        .bind(row.issued_at)
        .bind(row.start_utc)
        .bind(row.end_utc)
        .bind(product_id)
        .bind(product_id)
        .bind(row.latest_product_timestamp_utc)
        .execute(&mut **tx)
        .await?;

        if done.rows_affected() > 0 {
            changes.push(PendingIncidentChange {
                key: row.key.clone(),
                action: if existed {
                    IncidentChangeAction::Updated
                } else {
                    IncidentChangeAction::Created
                },
            });
        }
    }

    Ok(changes)
}
