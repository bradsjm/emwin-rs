//! Build persistence artifacts for completed files.
//!
//! The file pipeline keeps filesystem-side persistence concerns separate from server ingest and
//! delivery so the same assembled payload can be retained in memory, exposed over HTTP, and
//! persisted asynchronously.

use chrono::{DateTime, Utc};
use crc32fast::Hasher;
use emwin_db::{BlobEntry, BlobRole, CompletedFileMetadata, PersistRequest};
use emwin_protocol::ingest::ProductOrigin;
use std::path::{Path, PathBuf};

use crate::error::LiveResult;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArchiveStoragePlan {
    payload_relative_path: String,
    metadata_relative_path: String,
    request_key: String,
}

/// Paths and metadata returned after a file plus sidecar have been written.
#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct CompletedFileRecord {
    pub(crate) path: String,
    pub(crate) metadata_path: String,
}

/// Persists an assembled file and returns its displayable path.
#[cfg(test)]
pub(crate) fn write_completed_file(
    output_dir: &Path,
    filename: &str,
    data: &[u8],
) -> LiveResult<String> {
    let target = output_dir.join(filename);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&target, data)?;
    Ok(target.to_string_lossy().to_string())
}

/// Returns the sibling `.JSON` sidecar path for a completed file.
pub(crate) fn metadata_sidecar_path(output_dir: &Path, filename: &str) -> PathBuf {
    let target = output_dir.join(filename);
    match target.extension() {
        Some(_) => target.with_extension("JSON"),
        None => {
            let mut path = target.into_os_string();
            path.push(".JSON");
            PathBuf::from(path)
        }
    }
}

#[cfg(test)]
pub(crate) fn write_completed_metadata_json(
    output_dir: &Path,
    filename: &str,
    metadata: &CompletedFileMetadata,
) -> LiveResult<String> {
    let target = metadata_sidecar_path(output_dir, filename);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&target, serde_json::to_vec_pretty(metadata)?)?;
    Ok(target.to_string_lossy().to_string())
}

pub(crate) fn metadata_json_bytes(metadata: &CompletedFileMetadata) -> LiveResult<Vec<u8>> {
    Ok(serde_json::to_vec_pretty(metadata)?)
}

pub(crate) fn build_completed_file_metadata(
    filename: &str,
    timestamp_utc: u64,
    origin: ProductOrigin,
    data: &[u8],
) -> CompletedFileMetadata {
    CompletedFileMetadata::build(filename, timestamp_utc, origin, data)
}

#[cfg(test)]
pub(crate) fn persist_completed_record(
    output_dir: &Path,
    filename: &str,
    data: &[u8],
    metadata: CompletedFileMetadata,
) -> LiveResult<CompletedFileRecord> {
    let path = write_completed_file(output_dir, filename, data)?;
    let metadata_path = write_completed_metadata_json(output_dir, filename, &metadata)?;
    Ok(CompletedFileRecord {
        path,
        metadata_path,
    })
}

pub(crate) fn build_persist_request(
    filename: &str,
    data: &[u8],
    metadata: CompletedFileMetadata,
) -> LiveResult<PersistRequest<CompletedFileMetadata>> {
    let plan = build_archive_storage_plan(filename, data, &metadata);
    let metadata_bytes = metadata_json_bytes(&metadata)?;

    Ok(PersistRequest {
        request_key: plan.request_key,
        metadata,
        blobs: vec![
            BlobEntry::new(
                BlobRole::Payload,
                plan.payload_relative_path,
                data.to_vec(),
                Some("application/octet-stream"),
            ),
            BlobEntry::new(
                BlobRole::MetadataSidecar,
                plan.metadata_relative_path,
                metadata_bytes,
                Some("application/json"),
            ),
        ],
    })
}

fn build_archive_storage_plan(
    filename: &str,
    data: &[u8],
    metadata: &CompletedFileMetadata,
) -> ArchiveStoragePlan {
    let timestamp = timestamp_utc(metadata.timestamp_utc);
    let origin = origin_segment(&metadata.origin);
    let office = metadata
        .product_summary
        .office
        .as_ref()
        .map(|office| office.code.to_string())
        .filter(|code| !code.is_empty())
        .unwrap_or_else(|| "UNK".to_string());
    let family = metadata.product_summary.family.unwrap_or("unknown");
    let basename = basename_segment(filename);
    let leaf = format!(
        "{}-{}-{}",
        timestamp.format("%Y%m%dT%H%M%SZ"),
        crc32_hex(data),
        basename
    );
    let payload_relative_path = format!(
        "{origin}/{}/{}/{}/{office}/{family}/{leaf}",
        timestamp.format("%Y"),
        timestamp.format("%m"),
        timestamp.format("%d")
    );
    let metadata_relative_path = metadata_sidecar_path(Path::new(""), &payload_relative_path)
        .to_string_lossy()
        .trim_start_matches('/')
        .to_string();

    ArchiveStoragePlan {
        request_key: payload_relative_path.clone(),
        payload_relative_path,
        metadata_relative_path,
    }
}

fn timestamp_utc(timestamp_utc: u64) -> DateTime<Utc> {
    DateTime::from_timestamp(i64::try_from(timestamp_utc).unwrap_or(0), 0)
        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
}

fn origin_segment(origin: &ProductOrigin) -> &'static str {
    match origin {
        ProductOrigin::Qbt => "qbt",
        ProductOrigin::WxWire { .. } => "wxwire",
        _ => "unknown",
    }
}

fn basename_segment(filename: &str) -> &str {
    filename
        .rsplit(['/', '\\'])
        .find(|segment| !segment.is_empty())
        .unwrap_or(filename)
}

fn crc32_hex(data: &[u8]) -> String {
    let mut hasher = Hasher::new();
    hasher.update(data);
    format!("{:08x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use crate::archive_postprocess::post_process_archive;
    use emwin_parser::ProductEnrichmentSource;
    use emwin_protocol::ingest::ProductOrigin;

    use super::{
        build_archive_storage_plan, build_completed_file_metadata, build_persist_request,
        metadata_sidecar_path, persist_completed_record, write_completed_metadata_json,
    };
    use std::path::{Path, PathBuf};

    #[test]
    fn completed_metadata_uses_header_enrichment_for_valid_text_products() {
        let metadata = build_completed_file_metadata(
            "AFDBOX.TXT",
            1704070800,
            ProductOrigin::Qbt,
            b"000 \nFXUS61 KBOX 022101\nAFDBOX\nBody\n",
        );

        assert_eq!(metadata.product.source, ProductEnrichmentSource::TextHeader);
        assert_eq!(metadata.product.pil.as_deref(), Some("AFD"));
        assert_eq!(metadata.product.title, Some("Area Forecast Discussion"));
        assert!(metadata.product.issues.is_empty());
    }

    #[test]
    fn completed_metadata_surfaces_text_parse_warnings() {
        let metadata = build_completed_file_metadata(
            "AFDBOX.TXT",
            1704070800,
            ProductOrigin::Qbt,
            b"000 \nINVALID HEADER\nAFDBOX\nBody\n",
        );

        assert_eq!(metadata.product.source, ProductEnrichmentSource::TextHeader);
        assert_eq!(
            metadata.product.issues.first().map(|value| value.code),
            Some("invalid_wmo_header")
        );
        assert!(metadata.product.header.is_none());
    }

    #[test]
    fn completed_metadata_treats_zip_framed_txt_payload_as_unknown_zip() {
        let metadata = build_completed_file_metadata(
            "TAFALLUS.TXT",
            1704070800,
            ProductOrigin::Qbt,
            b"PK\x03\x04compressed bytes",
        );

        assert_eq!(metadata.product.source, ProductEnrichmentSource::Unknown);
        assert_eq!(metadata.product.container, "zip");
        assert!(metadata.product.issues.is_empty());
        assert!(metadata.product.header.is_none());
    }

    #[test]
    fn completed_metadata_parses_extracted_archive_content_like_plain_text() {
        let archive = {
            let cursor = std::io::Cursor::new(Vec::new());
            let mut writer = zip::ZipWriter::new(cursor);
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            use std::io::Write;
            writer
                .start_file("nested/AFDBOX.TXT", options)
                .expect("start file should succeed");
            writer
                .write_all(b"000 \nFXUS61 KBOX 022101\nAFDBOX\nBody\n")
                .expect("write should succeed");
            writer.finish().expect("finish should succeed").into_inner()
        };
        let delivered = post_process_archive(true, "AFDBOX.ZIP", &archive)
            .expect("archive post-processing should succeed");

        let metadata = build_completed_file_metadata(
            &delivered.filename,
            1704070800,
            ProductOrigin::Qbt,
            &delivered.data,
        );

        assert_eq!(metadata.filename, "nested/AFDBOX.TXT");
        assert_eq!(metadata.product.source, ProductEnrichmentSource::TextHeader);
        assert_eq!(metadata.product.pil.as_deref(), Some("AFD"));
        assert_eq!(metadata.product.container, "raw");
    }

    #[test]
    fn metadata_sidecar_paths_use_json_suffix() {
        for filename in ["nested/AFDBOX.TXT", "nested/AFDBOX"] {
            let path = metadata_sidecar_path(Path::new("/tmp/out"), filename);
            assert_eq!(path, PathBuf::from("/tmp/out/nested/AFDBOX.JSON"));
        }
    }

    #[test]
    fn metadata_json_round_trips() {
        let tmp = tempfile::tempdir().expect("tempdir should exist");
        let metadata = build_completed_file_metadata(
            "AFDBOX.TXT",
            1704070800,
            ProductOrigin::Qbt,
            b"000 \nINVALID HEADER\nAFDBOX\nBody\n",
        );

        let path = write_completed_metadata_json(tmp.path(), "AFDBOX.TXT", &metadata)
            .expect("metadata should write");
        let decoded: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).expect("metadata file should be readable"))
                .expect("metadata json should decode");

        assert_eq!(decoded["filename"], "AFDBOX.TXT");
        assert_eq!(decoded["size"], metadata.size);
        assert_eq!(decoded["product"]["schema_version"], 2);
        assert_eq!(
            decoded["product"]["issues"][0]["code"],
            "invalid_wmo_header"
        );
        assert!(decoded["product"].get("parsed").is_none());
    }

    #[test]
    fn build_persist_request_uses_partitioned_archival_paths() {
        let metadata = build_completed_file_metadata(
            "AFDBOX.TXT",
            1704070800,
            ProductOrigin::Qbt,
            b"000 \nFXUS61 KBOX 022101\nAFDBOX\nBody\n",
        );

        let request = build_persist_request(
            "AFDBOX.TXT",
            b"000 \nFXUS61 KBOX 022101\nAFDBOX\nBody\n",
            metadata,
        )
        .expect("persist request should build");

        assert_eq!(
            request.request_key,
            "qbt/2024/01/01/BOX/nws_text_product/20240101T010000Z-3addeb79-AFDBOX.TXT"
        );
        assert_eq!(request.blobs[0].relative_path, request.request_key);
        assert_eq!(
            request.blobs[1].relative_path,
            "qbt/2024/01/01/BOX/nws_text_product/20240101T010000Z-3addeb79-AFDBOX.JSON"
        );
    }

    #[test]
    fn archive_storage_plan_flattens_nested_filename_to_basename() {
        let metadata = build_completed_file_metadata(
            "nested/TAFPDKGA.TXT",
            1,
            ProductOrigin::Qbt,
            b"000 \nFTUS42 KFFC 022320\nTAFPDK\nBody\n",
        );

        let plan = build_archive_storage_plan(
            "nested/TAFPDKGA.TXT",
            b"000 \nFTUS42 KFFC 022320\nTAFPDK\nBody\n",
            &metadata,
        );

        assert_eq!(
            plan.payload_relative_path,
            "qbt/1970/01/01/FFC/nws_text_product/19700101T000001Z-e56e022c-TAFPDKGA.TXT"
        );
        assert_eq!(
            plan.metadata_relative_path,
            "qbt/1970/01/01/FFC/nws_text_product/19700101T000001Z-e56e022c-TAFPDKGA.JSON"
        );
    }

    #[test]
    fn archive_storage_plan_uses_fallback_segments_when_metadata_is_missing() {
        let metadata =
            build_completed_file_metadata("nested/UNKNOWN.BIN", 0, ProductOrigin::Qbt, b"payload");

        let plan = build_archive_storage_plan("nested/UNKNOWN.BIN", b"payload", &metadata);

        assert_eq!(
            plan.payload_relative_path,
            "qbt/1970/01/01/UNK/unknown/19700101T000000Z-422c6a15-UNKNOWN.BIN"
        );
    }

    #[test]
    fn persist_completed_record_writes_payload_and_metadata_sidecar() {
        let tmp = tempfile::tempdir().expect("tempdir should exist");
        let payload = b"000 \nFXUS61 KBOX 022101\nAFDBOX\nBody\n";
        let metadata = build_completed_file_metadata(
            "nested/AFDBOX.TXT",
            1704070800,
            ProductOrigin::Qbt,
            payload,
        );

        let record = persist_completed_record(tmp.path(), "nested/AFDBOX.TXT", payload, metadata)
            .expect("completed record should persist");

        assert_eq!(
            std::fs::read(&record.path).expect("payload file should be readable"),
            payload
        );

        let sidecar: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&record.metadata_path).expect("metadata file should be readable"),
        )
        .expect("metadata json should decode");

        assert_eq!(sidecar["filename"], "nested/AFDBOX.TXT");
        assert_eq!(
            PathBuf::from(&record.metadata_path),
            tmp.path().join("nested/AFDBOX.JSON")
        );
    }

    #[test]
    fn persist_completed_record_uses_extracted_filename_and_bytes() {
        let archive = {
            let cursor = std::io::Cursor::new(Vec::new());
            let mut writer = zip::ZipWriter::new(cursor);
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            use std::io::Write;
            writer
                .start_file("nested/AFDBOX.TXT", options)
                .expect("start file should succeed");
            writer
                .write_all(b"000 \nFXUS61 KBOX 022101\nAFDBOX\nBody\n")
                .expect("write should succeed");
            writer.finish().expect("finish should succeed").into_inner()
        };
        let delivered = post_process_archive(true, "AFDBOX.ZIP", &archive)
            .expect("archive post-processing should succeed");
        let metadata = build_completed_file_metadata(
            &delivered.filename,
            1704070800,
            ProductOrigin::Qbt,
            &delivered.data,
        );
        let tmp = tempfile::tempdir().expect("tempdir should exist");

        let record =
            persist_completed_record(tmp.path(), &delivered.filename, &delivered.data, metadata)
                .expect("completed record should persist");

        assert_eq!(
            std::fs::read(&record.path).expect("payload should be readable"),
            b"000 \nFXUS61 KBOX 022101\nAFDBOX\nBody\n"
        );
        assert_eq!(
            PathBuf::from(&record.path),
            tmp.path().join("nested/AFDBOX.TXT")
        );
        assert_eq!(
            PathBuf::from(&record.metadata_path),
            tmp.path().join("nested/AFDBOX.JSON")
        );
    }
}
