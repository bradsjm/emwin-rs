use crate::error::{PersistError, PersistResult};
use object_store::path::Path as ObjectStorePath;
use object_store::{ObjectStore, ObjectStoreExt, parse_url_opts};
use std::future::Future;
use std::io::ErrorKind;
use std::pin::Pin;
use std::sync::Arc;
use url::Url;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One blob to be written by a storage backend.
#[derive(Debug, Clone)]
pub struct BlobEntry {
    /// Semantic role assigned by the enqueue request.
    pub role: BlobRole,
    /// Backend-relative path for the object.
    pub relative_path: String,
    /// Raw blob bytes to persist.
    pub bytes: Vec<u8>,
    /// Optional MIME type attached to the blob.
    pub content_type: Option<String>,
}

/// Semantic role of a persisted blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobRole {
    /// Primary delivered payload bytes.
    Payload,
    /// JSON metadata sidecar for compatibility with filesystem consumers.
    MetadataSidecar,
}

impl BlobEntry {
    /// Builds a blob entry using a backend-relative path and optional content type.
    pub fn new(
        role: BlobRole,
        relative_path: impl Into<String>,
        bytes: Vec<u8>,
        content_type: Option<&str>,
    ) -> Self {
        Self {
            role,
            relative_path: relative_path.into(),
            bytes,
            content_type: content_type.map(str::to_string),
        }
    }
}

/// Stable reference returned after a blob has been persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBlob {
    /// Semantic role assigned by the enqueue request.
    pub role: BlobRole,
    /// Stable backend-specific location for later lookup.
    pub location: String,
    /// Number of persisted bytes.
    pub size_bytes: usize,
    /// Optional MIME type propagated from the enqueue request.
    pub content_type: Option<String>,
}

/// Reads persisted blobs without exposing transport details to callers.
#[derive(Debug, Default, Clone)]
pub(crate) struct StorageBlobReader;

impl StorageBlobReader {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) async fn read(&self, location: &str) -> PersistResult<Vec<u8>> {
        read_object_store_object(location).await
    }
}

/// Writes raw payload blobs and returns stable references for metadata storage.
pub trait BlobWriter: Send + Sync + 'static {
    /// Persists a blob entry and returns the resulting storage reference.
    fn write<'a>(&'a self, entry: &'a BlobEntry) -> BoxFuture<'a, PersistResult<StoredBlob>>;

    /// Deletes a previously persisted blob when storage-level cleanup is required.
    fn delete<'a>(&'a self, blob: &'a StoredBlob) -> BoxFuture<'a, PersistResult<()>>;

    /// Stable backend label for diagnostics.
    fn backend_name(&self) -> &'static str {
        "blob"
    }

    /// Human-readable target description for diagnostics.
    fn target_description(&self) -> String {
        "unavailable".to_string()
    }
}

impl<T> BlobWriter for Box<T>
where
    T: BlobWriter + ?Sized,
{
    fn write<'a>(&'a self, entry: &'a BlobEntry) -> BoxFuture<'a, PersistResult<StoredBlob>> {
        (**self).write(entry)
    }

    fn delete<'a>(&'a self, blob: &'a StoredBlob) -> BoxFuture<'a, PersistResult<()>> {
        (**self).delete(blob)
    }

    fn backend_name(&self) -> &'static str {
        (**self).backend_name()
    }

    fn target_description(&self) -> String {
        (**self).target_description()
    }
}

/// Object-store-backed blob writer rooted at a configured URI and optional path prefix.
#[derive(Debug, Clone)]
pub struct ObjectStoreBlobWriter {
    store: Arc<dyn ObjectStore>,
    prefix: Option<String>,
    root_location: String,
}

impl ObjectStoreBlobWriter {
    /// Creates an object store writer using env-driven configuration for the target URI scheme.
    pub fn new(root_url: Url) -> PersistResult<Self> {
        if root_url.query().is_some() || root_url.fragment().is_some() {
            return Err(PersistError::InvalidConfig(format!(
                "object store URI must not include query or fragment components: `{root_url}`"
            )));
        }

        let (store, path) = parse_url_opts(&root_url, std::env::vars()).map_err(|err| {
            PersistError::InvalidConfig(format!(
                "invalid object store config for `{root_url}`: {err}"
            ))
        })?;

        Ok(Self {
            store: Arc::from(store),
            prefix: path_prefix(&path),
            root_location: normalize_object_store_root(&root_url),
        })
    }

    fn describe_target(&self) -> String {
        self.root_location.clone()
    }
}

impl BlobWriter for ObjectStoreBlobWriter {
    fn write<'a>(&'a self, entry: &'a BlobEntry) -> BoxFuture<'a, PersistResult<StoredBlob>> {
        let store = Arc::clone(&self.store);
        let prefix = self.prefix.clone();
        let root_location = self.root_location.clone();
        let content_type = entry.content_type.clone();
        let size_bytes = entry.bytes.len();
        let role = entry.role;
        let bytes = entry.bytes.clone();
        let relative_path = entry.relative_path.clone();

        Box::pin(async move {
            let key = build_object_key(prefix.as_deref(), &relative_path);
            let path = parse_object_store_path(&key)?;
            store
                .put(&path, bytes.into())
                .await
                .map_err(|err| object_store_error_from_object_store("put_object", err, None))?;

            Ok(StoredBlob {
                role,
                location: format_object_store_location(&root_location, &relative_path),
                size_bytes,
                content_type,
            })
        })
    }

    fn delete<'a>(&'a self, blob: &'a StoredBlob) -> BoxFuture<'a, PersistResult<()>> {
        let location = blob.location.clone();
        let store = Arc::clone(&self.store);
        let prefix = self.prefix.clone();
        let root_location = self.root_location.clone();

        Box::pin(async move {
            let relative = strip_object_store_root(&location, &root_location)
                .map_err(PersistError::InvalidRequest)?;
            let key = build_object_key(prefix.as_deref(), &relative);
            let path = parse_object_store_path(&key)?;
            match store.delete(&path).await {
                Ok(()) => Ok(()),
                Err(object_store::Error::NotFound { .. }) => Ok(()),
                Err(err) => Err(object_store_error_from_object_store(
                    "delete_object",
                    err,
                    Some(&location),
                )),
            }
        })
    }

    fn backend_name(&self) -> &'static str {
        "object_store"
    }

    fn target_description(&self) -> String {
        self.describe_target()
    }
}

fn path_prefix(path: &ObjectStorePath) -> Option<String> {
    (!path.as_ref().is_empty()).then(|| path.to_string())
}

fn normalize_object_store_root(url: &Url) -> String {
    let mut normalized = url.clone();
    let trimmed_path = normalized.path().trim_end_matches('/').to_string();
    normalized.set_path(&trimmed_path);
    normalized.to_string()
}

#[cfg(test)]
pub(crate) fn normalize_prefix(prefix: Option<String>) -> Option<String> {
    prefix.and_then(|value| {
        let trimmed = value.trim_matches('/');
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn normalize_relative_path(relative_path: &str) -> String {
    relative_path
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string()
}

fn parse_object_store_path(path: &str) -> PersistResult<ObjectStorePath> {
    ObjectStorePath::parse(path).map_err(|err| PersistError::InvalidRequest(err.to_string()))
}

fn object_store_error_from_object_store(
    operation: &'static str,
    err: object_store::Error,
    location: Option<&str>,
) -> PersistError {
    match err {
        object_store::Error::NotFound { path, .. } => PersistError::Io(std::io::Error::new(
            ErrorKind::NotFound,
            format!(
                "stored blob `{}` was not found during {operation}",
                location.unwrap_or(&path)
            ),
        )),
        object_store::Error::InvalidPath { source } => {
            PersistError::InvalidRequest(source.to_string())
        }
        object_store::Error::NotSupported { source } => {
            PersistError::object_store(operation, false, source.to_string())
        }
        object_store::Error::AlreadyExists { source, .. }
        | object_store::Error::Precondition { source, .. }
        | object_store::Error::NotModified { source, .. } => {
            PersistError::object_store(operation, false, source.to_string())
        }
        object_store::Error::NotImplemented {
            operation: not_implemented,
            implementer,
        } => PersistError::object_store(
            operation,
            false,
            format!("{not_implemented} by {implementer}"),
        ),
        object_store::Error::JoinError { source } => {
            PersistError::object_store(operation, true, source.to_string())
        }
        object_store::Error::PermissionDenied { source, .. } => {
            PersistError::object_store(operation, false, source.to_string())
        }
        object_store::Error::Unauthenticated { source, .. } => {
            PersistError::object_store(operation, false, source.to_string())
        }
        object_store::Error::UnknownConfigurationKey { key, .. } => {
            PersistError::object_store(operation, false, format!("unknown config key `{key}`"))
        }
        other => PersistError::object_store(operation, true, other.to_string()),
    }
}

pub(crate) fn build_object_key(prefix: Option<&str>, relative_path: &str) -> String {
    let normalized_relative_path = normalize_relative_path(relative_path);
    match prefix {
        Some(prefix) if !prefix.is_empty() => format!("{prefix}/{normalized_relative_path}"),
        _ => normalized_relative_path,
    }
}

pub(crate) fn format_object_store_location(root_location: &str, relative_path: &str) -> String {
    let normalized_relative_path = normalize_relative_path(relative_path);
    match root_location.trim_end_matches('/') {
        "" => normalized_relative_path,
        root => format!("{root}/{normalized_relative_path}"),
    }
}

pub(crate) fn strip_object_store_root(
    location: &str,
    root_location: &str,
) -> Result<String, String> {
    let prefix = format!("{}/", root_location.trim_end_matches('/'));
    location
        .strip_prefix(&prefix)
        .map(normalize_relative_path)
        .ok_or_else(|| {
            format!(
                "stored blob location `{location}` does not belong to configured object store root `{root_location}`"
            )
        })
}

async fn read_object_store_object(location: &str) -> PersistResult<Vec<u8>> {
    let url = Url::parse(location).map_err(|err| {
        PersistError::InvalidRequest(format!(
            "invalid stored object store location `{location}`: {err}"
        ))
    })?;
    let (store, path) = parse_url_opts(&url, std::env::vars()).map_err(|err| {
        PersistError::InvalidRequest(format!(
            "invalid stored object store location `{location}`: {err}"
        ))
    })?;
    let response = store
        .get(&path)
        .await
        .map_err(|err| object_store_error_from_object_store("get_object", err, Some(location)))?;
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|err| PersistError::object_store("get_object", true, err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{
        ObjectStoreBlobWriter, build_object_key, format_object_store_location,
        normalize_object_store_root, normalize_prefix, strip_object_store_root,
    };
    use crate::{BlobEntry, BlobRole, BlobWriter};
    use url::Url;

    #[test]
    fn object_key_joining_normalizes_prefix_and_relative_path() {
        assert_eq!(
            normalize_prefix(Some("/archive/weather/".to_string())),
            Some("archive/weather".to_string())
        );
        assert_eq!(
            build_object_key(Some("archive/weather"), "nested\\AFDBOX.TXT"),
            "archive/weather/nested/AFDBOX.TXT"
        );
        assert_eq!(build_object_key(None, "/AFDBOX.TXT"), "AFDBOX.TXT");
    }

    #[test]
    fn object_store_locations_round_trip_with_root_prefix() {
        let location =
            format_object_store_location("s3://example-bucket/archive", "nested/AFDBOX.TXT");
        assert_eq!(location, "s3://example-bucket/archive/nested/AFDBOX.TXT");
        assert_eq!(
            strip_object_store_root(&location, "s3://example-bucket/archive")
                .expect("location should parse"),
            "nested/AFDBOX.TXT"
        );
    }

    #[test]
    fn object_store_location_rejects_other_roots() {
        let err = strip_object_store_root(
            "s3://other-bucket/archive/AFDBOX.TXT",
            "s3://example-bucket/archive",
        )
        .expect_err("root mismatch should fail");
        assert!(err.contains("does not belong to configured object store root"));
    }

    #[test]
    fn object_store_root_normalizes_trailing_slashes() {
        let url = Url::parse("https://storage.example.com/archive/").expect("url should parse");
        assert_eq!(
            normalize_object_store_root(&url),
            "https://storage.example.com/archive"
        );
    }

    #[tokio::test]
    async fn file_writer_persists_file_urls() {
        let temp = tempfile::tempdir().expect("tempdir should exist");
        let root_url = Url::from_directory_path(temp.path()).expect("directory url should build");
        let writer = ObjectStoreBlobWriter::new(root_url.clone()).expect("writer should build");
        let entry = BlobEntry::new(
            BlobRole::Payload,
            "archive/payload.txt",
            b"payload".to_vec(),
            Some("text/plain"),
        );

        let stored = writer.write(&entry).await.expect("write should succeed");

        assert_eq!(
            stored.location,
            format!(
                "{}/archive/payload.txt",
                root_url.as_str().trim_end_matches('/')
            )
        );
        let payload_path = temp.path().join("archive/payload.txt");
        assert_eq!(
            std::fs::read_to_string(payload_path).expect("payload should exist"),
            "payload"
        );
    }

    #[test]
    fn object_store_writer_uses_parsed_prefix_for_target_description() {
        let writer = ObjectStoreBlobWriter::new(
            Url::parse("s3://example-bucket/archive/weather/").expect("url should parse"),
        )
        .expect("writer should build");

        assert_eq!(
            writer.target_description(),
            "s3://example-bucket/archive/weather"
        );
    }

    #[test]
    fn object_store_writer_rejects_query_and_fragment_components() {
        let err = ObjectStoreBlobWriter::new(
            Url::parse("s3://example-bucket/archive?x=1").expect("url should parse"),
        )
        .expect_err("query string should fail");
        assert!(
            err.to_string()
                .contains("must not include query or fragment")
        );
    }
}
