mod common;

use common::*;
use emwin_db::{BlobRole, BlobWriter, ObjectStoreBlobWriter};
use url::Url;

#[tokio::test]
async fn archive_payload_reads_filesystem_backed_bytes() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let temp = tempfile::tempdir().expect("tempdir should exist");
    let payload_path = temp.path().join("payload.txt");
    let sidecar_path = temp.path().join("payload.JSON");
    std::fs::write(&payload_path, b"raw archive body").expect("payload write should succeed");
    std::fs::write(&sidecar_path, b"{}").expect("sidecar write should succeed");

    let sample = sample_case();
    let metadata = sample.metadata;
    let incident_key = sample.incident_key;
    cleanup_rows(&sink, &[&metadata.filename], &[incident_key]).await;

    let product_id = persist_metadata_with_blobs(
        &sink,
        metadata.clone(),
        sample_filesystem_blobs_at(
            Url::from_file_path(&payload_path)
                .expect("payload file url should build")
                .as_str(),
            Url::from_file_path(&sidecar_path)
                .expect("sidecar file url should build")
                .as_str(),
        ),
    )
    .await;

    let payload = sink
        .read_archived_payload(product_id)
        .await
        .expect("raw payload read should succeed")
        .expect("raw payload should exist");
    assert_eq!(payload.filename, metadata.filename);
    assert_eq!(payload.bytes, b"raw archive body");

    cleanup_rows(&sink, &[&metadata.filename], &[incident_key]).await;
}

#[tokio::test]
async fn archive_payload_reads_file_url_backed_bytes() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let temp = tempfile::tempdir().expect("tempdir should exist");
    let writer = ObjectStoreBlobWriter::new(
        Url::from_directory_path(temp.path()).expect("directory url should build"),
    )
    .expect("writer should build");
    let payload_entry = emwin_db::BlobEntry::new(
        BlobRole::Payload,
        "archive/payload.txt",
        b"raw archive body".to_vec(),
        Some("text/plain"),
    );
    let sidecar_entry = emwin_db::BlobEntry::new(
        BlobRole::MetadataSidecar,
        "archive/payload.JSON",
        b"{}".to_vec(),
        Some("application/json"),
    );
    let payload_blob = writer
        .write(&payload_entry)
        .await
        .expect("payload write should succeed");
    let sidecar_blob = writer
        .write(&sidecar_entry)
        .await
        .expect("sidecar write should succeed");
    assert!(payload_blob.location.starts_with("file://"));

    let sample = sample_case();
    let metadata = sample.metadata;
    let incident_key = sample.incident_key;
    cleanup_rows(&sink, &[&metadata.filename], &[incident_key]).await;

    let product_id =
        persist_metadata_with_blobs(&sink, metadata.clone(), vec![payload_blob, sidecar_blob])
            .await;

    let payload = sink
        .read_archived_payload(product_id)
        .await
        .expect("raw payload read should succeed")
        .expect("raw payload should exist");
    assert_eq!(payload.filename, metadata.filename);
    assert_eq!(payload.bytes, b"raw archive body");

    cleanup_rows(&sink, &[&metadata.filename], &[incident_key]).await;
}
