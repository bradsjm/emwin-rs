use crate::error::{PersistError, PersistResult};
use object_store::aws::AmazonS3Builder;
use object_store::local::LocalFileSystem;
use object_store::path::Path as ObjectStorePath;
use object_store::{ObjectStore, ObjectStoreExt};
use s3::Bucket;
use s3::bucket_ops::BucketConfiguration;
use s3::creds::Credentials;
use s3::region::Region;
use std::collections::BTreeMap;
use std::future::Future;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One blob to be written by a storage backend.
#[derive(Debug, Clone)]
pub struct BlobEntry {
    pub role: BlobRole,
    pub relative_path: String,
    pub bytes: Vec<u8>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobStorageKind {
    /// Blob stored on a local or mounted filesystem.
    Filesystem,
    /// Blob stored in Amazon S3-compatible object storage.
    S3,
}

impl BlobStorageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Filesystem => "filesystem",
            Self::S3 => "s3",
        }
    }
}

/// Stable reference returned after a blob has been persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBlob {
    /// Storage backend that accepted the blob.
    pub kind: BlobStorageKind,
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

    pub(crate) async fn read(
        &self,
        kind: BlobStorageKind,
        location: &str,
    ) -> PersistResult<Vec<u8>> {
        match kind {
            BlobStorageKind::Filesystem => tokio::fs::read(location).await.map_err(Into::into),
            BlobStorageKind::S3 => read_s3_object(location).await,
        }
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

/// Filesystem-backed blob writer rooted at a configured directory.
#[derive(Debug, Clone)]
pub struct FilesystemBlobWriter {
    root: PathBuf,
    store: LocalFileSystem,
}

impl FilesystemBlobWriter {
    /// Creates a filesystem writer rooted at the provided directory.
    pub fn new(root: PathBuf) -> Self {
        let root = normalize_filesystem_root(root);
        std::fs::create_dir_all(&root).expect("filesystem blob writer root must be creatable");
        Self {
            store: LocalFileSystem::new_with_prefix(&root)
                .expect("filesystem blob writer root must be readable"),
            root,
        }
    }

    fn describe_target(&self) -> String {
        self.root.display().to_string()
    }
}

fn normalize_filesystem_root(root: PathBuf) -> PathBuf {
    if root.is_absolute() {
        return root;
    }

    std::env::current_dir()
        .map(|cwd| cwd.join(&root))
        .unwrap_or(root)
}

/// S3-backed blob writer rooted at a bucket and optional prefix.
#[derive(Debug, Clone)]
pub struct S3BlobWriter {
    state: Arc<S3WriterState>,
    prefix: Option<String>,
    store: Arc<dyn ObjectStore>,
}

#[derive(Debug)]
struct S3WriterState {
    config: ResolvedS3Config,
    bucket: Box<Bucket>,
    readiness: Mutex<S3BucketReadiness>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum S3BucketReadiness {
    Unknown,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct S3Environment {
    region: Option<String>,
    default_region: Option<String>,
    endpoint_url: Option<String>,
    profile: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedS3Config {
    bucket_name: String,
    prefix: Option<String>,
    region: Region,
    path_style: bool,
    profile: Option<String>,
}

impl S3BlobWriter {
    /// Creates an S3 writer using env-driven AWS-compatible configuration.
    pub fn new(bucket_name: String, prefix: Option<String>) -> PersistResult<Self> {
        let config = resolve_s3_config(bucket_name, prefix, &S3Environment::from_process())?;
        let normalized_prefix = config.prefix.clone();
        let bucket = build_bucket(&config)?;
        let store = build_object_store(&config)?;
        Ok(Self {
            state: Arc::new(S3WriterState {
                config,
                bucket,
                readiness: Mutex::new(S3BucketReadiness::Unknown),
            }),
            prefix: normalized_prefix,
            store,
        })
    }

    async fn ensure_bucket_ready(&self) -> PersistResult<Box<Bucket>> {
        if *self
            .state
            .readiness
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            == S3BucketReadiness::Ready
        {
            return Ok(self.state.bucket.clone());
        }

        let bucket = self.state.bucket.clone();
        if bucket
            .exists()
            .await
            .map_err(|err| PersistError::s3_client("check_bucket", &err))?
        {
            self.mark_bucket_ready();
            return Ok(bucket);
        }

        let credentials =
            Credentials::new(None, None, None, None, self.state.config.profile.as_deref())
                .map_err(|err| {
                    PersistError::InvalidConfig(format!("invalid S3 credentials: {err}"))
                })?;

        let response = if self.state.config.path_style {
            Bucket::create_with_path_style(
                &self.state.config.bucket_name,
                self.state.config.region.clone(),
                credentials,
                BucketConfiguration::default(),
            )
            .await
            .map_err(|err| PersistError::s3_client("create_bucket", &err))?
        } else {
            Bucket::create(
                &self.state.config.bucket_name,
                self.state.config.region.clone(),
                credentials,
                BucketConfiguration::default(),
            )
            .await
            .map_err(|err| PersistError::s3_client("create_bucket", &err))?
        };

        if !matches!(response.response_code, 200 | 409) {
            return Err(PersistError::s3_response(
                "create_bucket",
                response.response_code,
                response.response_text,
            ));
        }

        self.mark_bucket_ready();
        Ok(self.state.bucket.clone())
    }

    fn mark_bucket_ready(&self) {
        *self
            .state
            .readiness
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = S3BucketReadiness::Ready;
    }

    fn reset_bucket_ready(&self) {
        *self
            .state
            .readiness
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = S3BucketReadiness::Unknown;
    }

    fn describe_target(&self) -> String {
        match self.prefix.as_deref() {
            Some(prefix) => format!("s3://{}/{}", self.state.config.bucket_name, prefix),
            None => format!("s3://{}", self.state.config.bucket_name),
        }
    }
}

impl S3Environment {
    pub(crate) fn from_process() -> Self {
        let vars = std::env::vars().collect::<BTreeMap<_, _>>();
        Self::from_map(&vars)
    }

    fn from_map(vars: &BTreeMap<String, String>) -> Self {
        Self {
            region: vars.get("AWS_REGION").cloned(),
            default_region: vars.get("AWS_DEFAULT_REGION").cloned(),
            endpoint_url: vars.get("AWS_ENDPOINT_URL").cloned(),
            profile: vars.get("AWS_PROFILE").cloned(),
        }
    }
}

impl BlobWriter for FilesystemBlobWriter {
    fn write<'a>(&'a self, entry: &'a BlobEntry) -> BoxFuture<'a, PersistResult<StoredBlob>> {
        let relative_path = entry.relative_path.clone();
        let store = self.store.clone();
        let content_type = entry.content_type.clone();
        Box::pin(async move {
            let path = parse_object_store_path(&normalize_relative_path(&relative_path))?;
            store
                .put(&path, entry.bytes.clone().into())
                .await
                .map_err(|err| filesystem_error_from_object_store("put_object", err))?;
            let location = store
                .path_to_filesystem(&path)
                .map_err(|err| PersistError::InvalidRequest(err.to_string()))?
                .to_string_lossy()
                .to_string();

            Ok(StoredBlob {
                kind: BlobStorageKind::Filesystem,
                role: entry.role,
                location,
                size_bytes: entry.bytes.len(),
                content_type,
            })
        })
    }

    fn delete<'a>(&'a self, blob: &'a StoredBlob) -> BoxFuture<'a, PersistResult<()>> {
        let location = blob.location.clone();
        let store = self.store.clone();
        let root = self.root.clone();
        Box::pin(async move {
            let relative = std::path::Path::new(&location)
                .strip_prefix(&root)
                .map_err(|_| {
                    PersistError::InvalidRequest(format!(
                        "stored blob location `{location}` does not belong to configured filesystem root"
                    ))
                })?;
            let path =
                parse_object_store_path(&normalize_relative_path(&relative.to_string_lossy()))?;
            match store.delete(&path).await {
                Ok(()) => Ok(()),
                Err(object_store::Error::NotFound { .. }) => Ok(()),
                Err(err) => Err(filesystem_error_from_object_store("delete_object", err)),
            }?;
            Ok(())
        })
    }

    fn backend_name(&self) -> &'static str {
        BlobStorageKind::Filesystem.as_str()
    }

    fn target_description(&self) -> String {
        self.describe_target()
    }
}

impl BlobWriter for S3BlobWriter {
    fn write<'a>(&'a self, entry: &'a BlobEntry) -> BoxFuture<'a, PersistResult<StoredBlob>> {
        let writer = self.clone();
        let key = build_object_key(self.prefix.as_deref(), &entry.relative_path);
        let content_type = entry.content_type.clone();
        let size_bytes = entry.bytes.len();
        let role = entry.role;
        let bytes = entry.bytes.clone();

        Box::pin(async move {
            let bucket = writer.ensure_bucket_ready().await?;
            let path = parse_object_store_path(&key)?;
            let put_result = writer.store.put(&path, bytes.into()).await;
            if let Err(err) = put_result {
                if matches!(err, object_store::Error::NotFound { .. }) {
                    writer.reset_bucket_ready();
                }
                return Err(s3_error_from_object_store("put_object", err));
            }

            Ok(StoredBlob {
                kind: BlobStorageKind::S3,
                role,
                location: format_s3_location(&bucket.name, &key),
                size_bytes,
                content_type,
            })
        })
    }

    fn delete<'a>(&'a self, blob: &'a StoredBlob) -> BoxFuture<'a, PersistResult<()>> {
        let location = blob.location.clone();
        let store = Arc::clone(&self.store);
        let bucket_name = self.state.config.bucket_name.clone();

        Box::pin(async move {
            let key = parse_s3_location(&location, &bucket_name)?;
            let path = parse_object_store_path(key)?;
            match store.delete(&path).await {
                Ok(_) => Ok(()),
                Err(object_store::Error::NotFound { .. }) => Ok(()),
                Err(err) => Err(s3_error_from_object_store("delete_object", err)),
            }
        })
    }

    fn backend_name(&self) -> &'static str {
        BlobStorageKind::S3.as_str()
    }

    fn target_description(&self) -> String {
        self.describe_target()
    }
}

pub(crate) fn resolve_s3_config(
    bucket_name: String,
    prefix: Option<String>,
    env: &S3Environment,
) -> PersistResult<ResolvedS3Config> {
    let endpoint_url = env
        .endpoint_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let has_custom_endpoint = endpoint_url.is_some();

    let region_name = env
        .region
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            env.default_region
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        });

    let region = match (has_custom_endpoint, region_name, endpoint_url.clone()) {
        (true, Some(region), Some(endpoint)) => Region::Custom { region, endpoint },
        (true, None, Some(endpoint)) => Region::Custom {
            region: "us-east-1".to_string(),
            endpoint,
        },
        (true, _, None) => unreachable!("custom endpoint mode requires endpoint URL"),
        (false, Some(region), _) => region.parse().map_err(|err| {
            PersistError::InvalidConfig(format!("invalid AWS region for S3 writer: {err}"))
        })?,
        (false, None, _) => {
            return Err(PersistError::InvalidConfig(
                "S3 output requires AWS_REGION or AWS_DEFAULT_REGION unless AWS_ENDPOINT_URL is set"
                    .to_string(),
            ));
        }
    };

    Ok(ResolvedS3Config {
        bucket_name,
        prefix: normalize_prefix(prefix),
        region,
        path_style: has_custom_endpoint,
        profile: env.profile.clone(),
    })
}

pub(crate) fn build_bucket(config: &ResolvedS3Config) -> PersistResult<Box<Bucket>> {
    let credentials = Credentials::new(None, None, None, None, config.profile.as_deref())
        .map_err(|err| PersistError::InvalidConfig(format!("invalid S3 credentials: {err}")))?;
    let bucket = Bucket::new(&config.bucket_name, config.region.clone(), credentials)
        .map_err(|err| PersistError::InvalidConfig(format!("invalid S3 config: {err}")))?;

    if config.path_style {
        Ok(bucket.with_path_style())
    } else {
        Ok(bucket)
    }
}

pub(crate) fn build_object_store(config: &ResolvedS3Config) -> PersistResult<Arc<dyn ObjectStore>> {
    let credentials = Credentials::new(None, None, None, None, config.profile.as_deref())
        .map_err(|err| PersistError::InvalidConfig(format!("invalid S3 credentials: {err}")))?;
    let mut builder = AmazonS3Builder::new()
        .with_bucket_name(&config.bucket_name)
        .with_region(region_name(&config.region))
        .with_virtual_hosted_style_request(!config.path_style);

    if let Some(access_key_id) = credentials.access_key {
        builder = builder.with_access_key_id(access_key_id);
    }
    if let Some(secret_access_key) = credentials.secret_key {
        builder = builder.with_secret_access_key(secret_access_key);
    }
    if let Some(token) = credentials.session_token.or(credentials.security_token) {
        builder = builder.with_token(token);
    }
    if let Region::Custom { endpoint, .. } = &config.region {
        builder = builder
            .with_endpoint(endpoint)
            .with_allow_http(endpoint.starts_with("http://"));
    }

    builder
        .build()
        .map(|store| Arc::new(store) as Arc<dyn ObjectStore>)
        .map_err(|err| {
            PersistError::InvalidConfig(format!("invalid S3 object store config: {err}"))
        })
}

pub(crate) fn normalize_prefix(prefix: Option<String>) -> Option<String> {
    prefix.and_then(|value| {
        let trimmed = value.trim_matches('/');
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn region_name(region: &Region) -> String {
    match region {
        Region::Custom { region, .. } => region.clone(),
        _ => region.to_string(),
    }
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

fn filesystem_error_from_object_store(
    operation: &'static str,
    err: object_store::Error,
) -> PersistError {
    match err {
        object_store::Error::NotFound { path, .. } => PersistError::Io(std::io::Error::new(
            ErrorKind::NotFound,
            format!("stored blob `{path}` was not found during {operation}"),
        )),
        object_store::Error::InvalidPath { source } => {
            PersistError::InvalidRequest(source.to_string())
        }
        other => PersistError::Io(std::io::Error::other(format!(
            "filesystem {operation} failed: {other}"
        ))),
    }
}

fn s3_error_from_object_store(operation: &'static str, err: object_store::Error) -> PersistError {
    match err {
        object_store::Error::NotFound { path, .. } => PersistError::Io(std::io::Error::new(
            ErrorKind::NotFound,
            format!("stored blob `s3://{path}` was not found during {operation}"),
        )),
        object_store::Error::InvalidPath { source } => {
            PersistError::InvalidRequest(source.to_string())
        }
        other => PersistError::S3 {
            operation,
            retryable: true,
            message: other.to_string(),
        },
    }
}

pub(crate) fn build_object_key(prefix: Option<&str>, relative_path: &str) -> String {
    let normalized_relative_path = normalize_relative_path(relative_path);
    match prefix {
        Some(prefix) if !prefix.is_empty() => format!("{prefix}/{normalized_relative_path}"),
        _ => normalized_relative_path,
    }
}

pub(crate) fn format_s3_location(bucket: &str, key: &str) -> String {
    format!("s3://{bucket}/{key}")
}

pub(crate) fn parse_s3_location<'a>(location: &'a str, bucket: &str) -> PersistResult<&'a str> {
    let prefix = format!("s3://{bucket}/");
    location.strip_prefix(&prefix).ok_or_else(|| {
        PersistError::InvalidRequest(format!(
            "stored blob location `{location}` does not belong to bucket `{bucket}`"
        ))
    })
}

async fn read_s3_object(location: &str) -> PersistResult<Vec<u8>> {
    let (bucket_name, _) = split_s3_location(location)?;
    let config = resolve_s3_config(bucket_name.clone(), None, &S3Environment::from_process())?;
    let key = parse_s3_location(location, &bucket_name)?;
    let store = build_object_store(&config)?;
    let path = parse_object_store_path(key)?;
    let response = store
        .get(&path)
        .await
        .map_err(|err| s3_error_from_object_store("get_object", err))?;
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|err| PersistError::S3 {
            operation: "get_object",
            retryable: true,
            message: err.to_string(),
        })
}

fn split_s3_location(location: &str) -> PersistResult<(String, String)> {
    let without_scheme = location.strip_prefix("s3://").ok_or_else(|| {
        PersistError::InvalidRequest(format!("invalid stored S3 location `{location}`"))
    })?;
    let (bucket, key) = without_scheme.split_once('/').ok_or_else(|| {
        PersistError::InvalidRequest(format!("invalid stored S3 location `{location}`"))
    })?;
    if bucket.is_empty() || key.is_empty() {
        return Err(PersistError::InvalidRequest(format!(
            "invalid stored S3 location `{location}`"
        )));
    }
    Ok((bucket.to_string(), key.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{
        FilesystemBlobWriter, ResolvedS3Config, S3BlobWriter, S3BucketReadiness, S3Environment,
        S3WriterState, build_object_key, format_s3_location, normalize_prefix, parse_s3_location,
        resolve_s3_config,
    };
    use crate::{BlobEntry, BlobRole, BlobStorageKind, BlobWriter};
    use s3::Bucket;
    use s3::region::Region;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    #[test]
    fn s3_key_joining_normalizes_prefix_and_relative_path() {
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
    fn s3_locations_round_trip_with_bucket_prefix() {
        let location = format_s3_location("example-bucket", "archive/AFDBOX.TXT");
        assert_eq!(location, "s3://example-bucket/archive/AFDBOX.TXT");
        assert_eq!(
            parse_s3_location(&location, "example-bucket").expect("location should parse"),
            "archive/AFDBOX.TXT"
        );
    }

    #[test]
    fn s3_location_rejects_other_buckets() {
        let err = parse_s3_location("s3://other-bucket/archive/AFDBOX.TXT", "example-bucket")
            .expect_err("bucket mismatch should fail");
        assert!(err.to_string().contains("does not belong to bucket"));
    }

    #[test]
    fn s3_resolver_uses_hosted_style_when_no_endpoint_is_set() {
        let env = env_with([(String::from("AWS_REGION"), String::from("us-west-2"))]);
        let config = resolve_s3_config("bucket".to_string(), Some("archive".to_string()), &env)
            .expect("hosted style config should resolve");

        assert!(matches!(config.region, Region::UsWest2));
        assert!(!config.path_style);
        assert_eq!(config.prefix.as_deref(), Some("archive"));
    }

    #[test]
    fn s3_resolver_uses_custom_endpoint_and_path_style() {
        let env = env_with([
            (
                String::from("AWS_ENDPOINT_URL"),
                String::from("http://localhost:9000"),
            ),
            (String::from("AWS_DEFAULT_REGION"), String::from("minio")),
        ]);
        let config = resolve_s3_config("bucket".to_string(), None, &env)
            .expect("custom endpoint config should resolve");

        match config.region {
            Region::Custom { region, endpoint } => {
                assert_eq!(region, "minio");
                assert_eq!(endpoint, "http://localhost:9000");
            }
            other => panic!("expected custom region, got {other:?}"),
        }
        assert!(config.path_style);
    }

    #[test]
    fn s3_resolver_falls_back_to_default_region_for_aws() {
        let env = env_with([(
            String::from("AWS_DEFAULT_REGION"),
            String::from("us-east-1"),
        )]);
        let config = resolve_s3_config("bucket".to_string(), None, &env)
            .expect("default region should resolve");

        assert!(matches!(config.region, Region::UsEast1));
    }

    #[test]
    fn s3_resolver_defaults_custom_endpoint_region_to_us_east_1() {
        let env = env_with([(
            String::from("AWS_ENDPOINT_URL"),
            String::from("http://localhost:9000"),
        )]);
        let config = resolve_s3_config("bucket".to_string(), None, &env)
            .expect("custom endpoint should default region");

        match config.region {
            Region::Custom { region, endpoint } => {
                assert_eq!(region, "us-east-1");
                assert_eq!(endpoint, "http://localhost:9000");
            }
            other => panic!("expected custom region, got {other:?}"),
        }
        assert!(config.path_style);
    }

    #[test]
    fn s3_resolver_requires_region_without_custom_endpoint() {
        let err = resolve_s3_config(
            "bucket".to_string(),
            None,
            &S3Environment::from_map(&BTreeMap::new()),
        )
        .expect_err("missing region should fail");
        assert!(err.to_string().contains("AWS_REGION or AWS_DEFAULT_REGION"));
    }

    #[tokio::test]
    async fn filesystem_writer_creates_deep_parent_directories() {
        let temp = tempfile::tempdir().expect("tempdir should exist");
        let writer = FilesystemBlobWriter::new(temp.path().to_path_buf());
        let entry = BlobEntry::new(
            BlobRole::Payload,
            "qbt/2026/03/16/BOX/nws_text_product/20260316T021530Z-deadbeef-AFDBOX.TXT",
            b"payload".to_vec(),
            Some("text/plain"),
        );

        let stored = writer.write(&entry).await.expect("write should succeed");

        assert_eq!(stored.kind, BlobStorageKind::Filesystem);
        assert_eq!(
            std::fs::read_to_string(temp.path().join(&entry.relative_path))
                .expect("payload should exist"),
            "payload"
        );
        assert!(
            std::path::Path::new(&stored.location).is_absolute(),
            "filesystem blob locations should be absolute"
        );
    }

    #[tokio::test]
    async fn filesystem_writer_normalizes_relative_roots_to_absolute_locations() {
        let temp = tempfile::Builder::new()
            .prefix("emwin-db-writer-relative-")
            .tempdir_in(".")
            .expect("tempdir in cwd should exist");
        let root = PathBuf::from(
            temp.path()
                .file_name()
                .expect("tempdir should have a file name"),
        );
        let writer = FilesystemBlobWriter::new(root.clone());
        let entry = BlobEntry::new(
            BlobRole::Payload,
            "archive/payload.txt",
            b"payload".to_vec(),
            Some("text/plain"),
        );

        let stored = writer.write(&entry).await.expect("write should succeed");

        assert!(
            std::path::Path::new(&stored.location).is_absolute(),
            "relative roots should be persisted as absolute blob locations"
        );
        assert_eq!(
            std::fs::read_to_string(std::path::Path::new(&stored.location))
                .expect("payload should exist at the stored location"),
            "payload"
        );
    }

    #[test]
    fn bucket_readiness_can_be_reset() {
        let writer = S3BlobWriter {
            state: Arc::new(S3WriterState {
                config: ResolvedS3Config {
                    bucket_name: "bucket".to_string(),
                    prefix: Some("archive".to_string()),
                    region: Region::UsEast1,
                    path_style: false,
                    profile: None,
                },
                bucket: Box::new(
                    Bucket::new_public("bucket", Region::UsEast1).expect("bucket should build"),
                ),
                readiness: Mutex::new(S3BucketReadiness::Ready),
            }),
            prefix: Some("archive".to_string()),
            store: Arc::new(object_store::memory::InMemory::new()),
        };

        writer.reset_bucket_ready();

        assert_eq!(
            *writer
                .state
                .readiness
                .lock()
                .expect("mutex should not be poisoned"),
            S3BucketReadiness::Unknown
        );
    }

    fn env_with<const N: usize>(entries: [(String, String); N]) -> S3Environment {
        S3Environment::from_map(&entries.into_iter().collect())
    }
}
