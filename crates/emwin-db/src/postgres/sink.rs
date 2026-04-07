use super::PostgresMetadataSink;
use super::{prepare, query, write};
use crate::error::PersistResult;
use crate::metadata::CompletedFileMetadata;
use crate::runtime::{MetadataSink, PersistedRequest};
use crate::writer::BoxFuture;
use emwin_service::{IncidentChange, IncidentChangeTrigger};

impl MetadataSink<CompletedFileMetadata> for PostgresMetadataSink {
    fn persist<'a>(
        &'a self,
        request: PersistedRequest<CompletedFileMetadata>,
    ) -> BoxFuture<'a, PersistResult<()>> {
        Box::pin(async move {
            let prepared = prepare::PreparedProduct::prepare(&request.metadata, &request.blobs)?;
            let pool = self.ensure_pool().await?;
            let result: PersistResult<Vec<IncidentChange>> = async {
                let mut tx = pool.begin().await?;
                let product_id = write::upsert_product(&mut tx, &prepared).await?;
                let incident_changes =
                    write::replace_children(&mut tx, product_id, &prepared).await?;
                tx.commit().await?;
                query::load_incident_changes(
                    &pool,
                    incident_changes,
                    IncidentChangeTrigger::Persist,
                )
                .await
            }
            .await;

            match result {
                Ok(incident_changes) => {
                    self.publish_incident_changes(incident_changes);
                    Ok(())
                }
                Err(err) => {
                    self.handle_runtime_error(&err).await;
                    Err(err)
                }
            }
        })
    }

    fn backend_name(&self) -> &'static str {
        "database"
    }

    fn target_description(&self) -> Option<String> {
        Some(self.describe_target())
    }
}
