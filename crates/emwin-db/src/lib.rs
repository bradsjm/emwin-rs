//! Async persistence runtime and pluggable blob storage backends.
//!
//! This crate keeps ingest producers off the storage hot path by queueing persistence requests for
//! a background worker. Producers enqueue completed products without awaiting filesystem, object
//! storage, or database I/O.
//!
//! # Features
//!
//! - **Filesystem**: Local file storage backend
//! - **S3**: AWS S3-compatible object storage backend
//! - **PostgreSQL**: Metadata persistence with Postgres
//!
//! # Example
//!
//! Queue completed products for filesystem persistence:
//!
//! ```no_run
//! use emwin_db::{
//!     BlobEntry, BlobRole, FilesystemBlobWriter, PersistRequest, PersistenceConfig,
//!     PersistenceRuntime, NoopMetadataSink,
//! };
//! use std::path::PathBuf;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = PersistenceConfig::new(1000);
//!     let writer = FilesystemBlobWriter::new(PathBuf::from("./out"));
//!     let metadata_sink = NoopMetadataSink;
//!
//!     let runtime = PersistenceRuntime::spawn(config, Box::new(writer), metadata_sink);
//!     let producer = runtime.producer();
//!
//!     let result = producer.enqueue(PersistRequest {
//!         request_key: "example.txt".to_string(),
//!         metadata: (),
//!         blobs: vec![
//!             BlobEntry::new(
//!                 BlobRole::Payload,
//!                 "example.txt",
//!                 b"product data".to_vec(),
//!                 Some("text/plain"),
//!             ),
//!         ],
//!     });
//!
//!     if !result.accepted {
//!         eprintln!("Request rejected or evicted: {:?}", result.evicted_oldest_key);
//!     }
//!
//!     runtime.shutdown().await?;
//!     Ok(())
//! }
//! ```

mod archive_filter;
mod error;
mod metadata;
mod postgres;
mod runtime;
mod writer;

pub use archive_filter::{
    ArchiveFilterInput, build_cell_aggregate_query, build_facet_aggregate_query,
    build_feature_list_query, build_timeseries_aggregate_query, parse_archive_bool,
};
pub use error::{PersistError, PersistResult};
pub use metadata::CompletedFileMetadata;
pub use postgres::{PostgresConfig, PostgresMetadataSink};
pub use runtime::{
    EnqueueResult, MetadataSink, NoopMetadataSink, PersistRequest, PersistedRequest,
    PersistenceConfig, PersistenceProducer, PersistenceRuntime, PersistenceStats,
};
pub use writer::{
    BlobEntry, BlobRole, BlobStorageKind, BlobWriter, FilesystemBlobWriter, S3BlobWriter,
    StoredBlob,
};
