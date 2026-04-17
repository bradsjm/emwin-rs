#![allow(missing_docs)]

use crate::{LiveRuntime, LiveTelemetry, SourceKind};

#[derive(Default)]
pub struct LiveRuntimeTestBuilder {
    retained_files: Vec<(String, Vec<u8>, u64, SourceKind)>,
    telemetry: Option<LiveTelemetry>,
    archive: Option<emwin_db::PostgresMetadataSink>,
    persistence: Option<emwin_db::PersistenceProducer<emwin_db::CompletedFileMetadata>>,
    upstream_endpoint: Option<String>,
    active_servers: usize,
    archive_status: Option<(String, u64, u64)>,
}

impl LiveRuntimeTestBuilder {
    pub fn retained_files(
        mut self,
        retained_files: Vec<(String, Vec<u8>, u64, SourceKind)>,
    ) -> Self {
        self.retained_files = retained_files;
        self
    }

    pub fn telemetry(mut self, telemetry: LiveTelemetry) -> Self {
        self.telemetry = Some(telemetry);
        self
    }

    pub fn archive(mut self, archive: emwin_db::PostgresMetadataSink) -> Self {
        self.archive = Some(archive);
        self
    }

    pub fn persistence(
        mut self,
        persistence: emwin_db::PersistenceProducer<emwin_db::CompletedFileMetadata>,
    ) -> Self {
        self.persistence = Some(persistence);
        self
    }

    pub fn upstream_endpoint(mut self, upstream_endpoint: impl Into<String>) -> Self {
        self.upstream_endpoint = Some(upstream_endpoint.into());
        self
    }

    pub fn active_servers(mut self, active_servers: usize) -> Self {
        self.active_servers = active_servers;
        self
    }

    pub fn archive_status(
        mut self,
        last_error: impl Into<String>,
        errors_total: u64,
        pool_timeouts_total: u64,
    ) -> Self {
        self.archive_status = Some((last_error.into(), errors_total, pool_timeouts_total));
        self
    }

    pub fn build(self) -> LiveRuntime {
        LiveRuntime::from_test_state(
            self.retained_files,
            self.telemetry.unwrap_or(LiveTelemetry::Unavailable),
            self.archive,
            self.persistence,
            self.upstream_endpoint,
            self.active_servers,
            self.archive_status,
        )
    }
}

pub fn runtime() -> LiveRuntimeTestBuilder {
    LiveRuntimeTestBuilder::default()
}
