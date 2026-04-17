//! Async persistence runtime and pluggable blob storage backends.
//!
//! This crate keeps ingest producers off the storage hot path by queueing persistence requests for
//! a background worker. Producers enqueue completed products without awaiting filesystem, object
//! storage, or database I/O.
//!
//! # Features
//!
//! - **Object Store**: URI-addressed object storage backends supported by `object_store`
//! - **PostgreSQL**: Metadata persistence with Postgres
//!
//! # Example
//!
//! Queue completed products for URI-based object-store persistence:
//!
//! ```no_run
//! use emwin_db::{
//!     BlobEntry, BlobRole, ObjectStoreBlobWriter, PersistRequest, PersistenceConfig,
//!     PersistenceRuntime, NoopMetadataSink,
//! };
//! use url::Url;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = PersistenceConfig::new(1000);
//!     let writer = ObjectStoreBlobWriter::new(Url::parse("file:///tmp/emwin-out")?)?;
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

mod error;
mod metadata;
mod postgres;
mod runtime;
mod writer;

pub use error::{PersistError, PersistResult};
pub use metadata::{CompletedFileMetadata, build_completed_file_metadata};
pub use postgres::{
    AlertContactPointRecord, DEFAULT_MAX_DB_CONNECTIONS, PostgresConfig, PostgresMetadataSink,
};
pub use runtime::{
    EnqueueResult, MetadataSink, NoopMetadataSink, PersistRequest, PersistedRequest,
    PersistenceConfig, PersistenceProducer, PersistenceRuntime, PersistenceStats,
};
pub use writer::{BlobEntry, BlobRole, BlobWriter, ObjectStoreBlobWriter, StoredBlob};
