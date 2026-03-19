mod common;

use common::*;
use emwin_db::{BlobRole, BlobWriter};

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

    let metadata = sample_metadata();
    let incident_key = TestIncidentKey {
        office: "KOAX",
        phenomena: "FF",
        significance: "W",
        etn: 1,
    };
    cleanup_rows(&sink, &[&metadata.filename], &[incident_key]).await;

    let product_id = persist_metadata_with_blobs(
        &sink,
        metadata.clone(),
        sample_filesystem_blobs_at(
            &payload_path.to_string_lossy(),
            &sidecar_path.to_string_lossy(),
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
async fn archive_payload_reads_writer_locations_from_relative_filesystem_roots() {
    let Some(sink) = connect_test_sink().await else {
        return;
    };

    let temp = tempfile::Builder::new()
        .prefix("emwin-db-archive-relative-")
        .tempdir_in(".")
        .expect("tempdir in cwd should exist");
    let root = std::path::PathBuf::from(
        temp.path()
            .file_name()
            .expect("tempdir should have a file name"),
    );
    let writer = emwin_db::FilesystemBlobWriter::new(root);
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
    assert!(
        std::path::Path::new(&payload_blob.location).is_absolute(),
        "relative filesystem roots should persist absolute payload locations"
    );

    let metadata = sample_metadata();
    let incident_key = TestIncidentKey {
        office: "KOAX",
        phenomena: "FF",
        significance: "W",
        etn: 2,
    };
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
