//! Archive post-processing for completed EMWIN products.
//!
//! When enabled, `.ZIP` and `.ZIS` payloads are unwrapped before downstream parsing so the
//! extracted product is treated the same as a non-archived payload.

use bytes::Bytes;
use std::io::Cursor;
use std::io::Read;
use std::path::Path;
use thiserror::Error;

/// Canonical delivered product after optional archive post-processing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeliveredProduct {
    pub(crate) filename: String,
    pub(crate) data: Bytes,
}

#[derive(Debug, Error)]
pub(crate) enum ArchivePostProcessError {
    #[error("invalid archive")]
    InvalidArchive(#[source] zip::result::ZipError),
    #[error("archive has no entries")]
    EmptyArchive,
    #[error("invalid archive entry")]
    InvalidEntry(#[source] zip::result::ZipError),
    #[error("archive entry is a directory")]
    DirectoryEntry,
    #[error("archive entry path is empty")]
    EmptyEntryPath,
    #[error("archive entry path is unsafe")]
    UnsafeEntryPath,
    #[error("failed to read archive entry")]
    ReadEntry(#[source] std::io::Error),
}

pub(crate) fn post_process_archive(
    enabled: bool,
    filename: &str,
    data: &[u8],
) -> Result<DeliveredProduct, ArchivePostProcessError> {
    if !enabled || !is_archive_filename(filename) {
        return Ok(DeliveredProduct {
            filename: filename.to_string(),
            data: Bytes::copy_from_slice(data),
        });
    }

    let mut archive =
        zip::ZipArchive::new(Cursor::new(data)).map_err(ArchivePostProcessError::InvalidArchive)?;
    if archive.is_empty() {
        return Err(ArchivePostProcessError::EmptyArchive);
    }

    let mut entry = archive
        .by_index(0)
        .map_err(ArchivePostProcessError::InvalidEntry)?;
    if entry.is_dir() {
        return Err(ArchivePostProcessError::DirectoryEntry);
    }

    let path = entry
        .enclosed_name()
        .ok_or(ArchivePostProcessError::UnsafeEntryPath)?;
    let delivered_filename = normalize_entry_path(&path)?;

    let mut extracted = Vec::new();
    entry
        .read_to_end(&mut extracted)
        .map_err(ArchivePostProcessError::ReadEntry)?;

    Ok(DeliveredProduct {
        filename: delivered_filename,
        data: Bytes::from(extracted),
    })
}

fn is_archive_filename(filename: &str) -> bool {
    filename
        .rsplit_once('.')
        .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("zip") || ext.eq_ignore_ascii_case("zis"))
}

fn normalize_entry_path(path: &Path) -> Result<String, ArchivePostProcessError> {
    if path.as_os_str().is_empty() {
        return Err(ArchivePostProcessError::EmptyEntryPath);
    }

    let filename = path.to_string_lossy().replace('\\', "/");
    if filename.is_empty() || filename.starts_with('/') {
        return Err(ArchivePostProcessError::UnsafeEntryPath);
    }

    Ok(filename)
}

#[cfg(test)]
mod tests {
    use super::{ArchivePostProcessError, post_process_archive};
    use std::io::Write;
    use zip::CompressionMethod;
    use zip::write::FileOptions;

    fn archive(entries: &[(&str, Option<&[u8]>)]) -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let options: FileOptions<'_, ()> =
            FileOptions::default().compression_method(CompressionMethod::Stored);
        for (name, body) in entries {
            if let Some(body) = body {
                writer
                    .start_file(name, options)
                    .expect("start file should succeed");
                writer.write_all(body).expect("write body should succeed");
            } else {
                writer
                    .add_directory(*name, options)
                    .expect("add directory should succeed");
            }
        }
        writer.finish().expect("finish should succeed").into_inner()
    }

    #[test]
    fn non_archive_filename_passes_through() {
        let delivered = post_process_archive(true, "AFDBOX.TXT", b"body")
            .expect("non-archive should pass through");

        assert_eq!(delivered.filename, "AFDBOX.TXT");
        assert_eq!(delivered.data.as_ref(), b"body");
    }

    #[test]
    fn archive_detection_is_case_insensitive_for_zip_and_zis() {
        let zip_bytes = archive(&[("nested/AFDBOX.TXT", Some(b"body"))]);

        let zip = post_process_archive(true, "AFDBOX.ZIP", &zip_bytes)
            .expect("zip extraction should succeed");
        let zis = post_process_archive(true, "AFDBOX.zIs", &zip_bytes)
            .expect("zis extraction should succeed");

        assert_eq!(zip.filename, "nested/AFDBOX.TXT");
        assert_eq!(zis.filename, "nested/AFDBOX.TXT");
    }

    #[test]
    fn extracts_first_entry_only() {
        let zip_bytes = archive(&[
            ("nested/FIRST.TXT", Some(b"first")),
            ("SECOND.TXT", Some(b"second")),
        ]);

        let delivered = post_process_archive(true, "AFD.ZIP", &zip_bytes)
            .expect("archive extraction should succeed");

        assert_eq!(delivered.filename, "nested/FIRST.TXT");
        assert_eq!(delivered.data.as_ref(), b"first");
    }

    #[test]
    fn rejects_empty_archive() {
        let zip_bytes = archive(&[]);

        let err = post_process_archive(true, "AFD.ZIP", &zip_bytes)
            .expect_err("empty archive should be rejected");

        assert!(matches!(err, ArchivePostProcessError::EmptyArchive));
    }

    #[test]
    fn rejects_invalid_zip_bytes() {
        let err = post_process_archive(true, "AFD.ZIP", b"not a zip")
            .expect_err("invalid archive should fail");

        assert!(matches!(err, ArchivePostProcessError::InvalidArchive(_)));
    }

    #[test]
    fn rejects_directory_first_entry() {
        let zip_bytes = archive(&[("nested/", None), ("nested/AFD.TXT", Some(b"body"))]);

        let err = post_process_archive(true, "AFD.ZIP", &zip_bytes)
            .expect_err("directory first entry should fail");

        assert!(matches!(err, ArchivePostProcessError::DirectoryEntry));
    }

    #[test]
    fn rejects_unsafe_first_entry_path() {
        let zip_bytes = archive(&[("../AFD.TXT", Some(b"body"))]);

        let err =
            post_process_archive(true, "AFD.ZIP", &zip_bytes).expect_err("unsafe path should fail");

        assert!(matches!(err, ArchivePostProcessError::UnsafeEntryPath));
    }

    #[test]
    fn accepts_safe_nested_path() {
        let zip_bytes = archive(&[("nested/AFD.TXT", Some(b"body"))]);

        let delivered = post_process_archive(true, "AFD.ZIP", &zip_bytes)
            .expect("safe nested path should succeed");

        assert_eq!(delivered.filename, "nested/AFD.TXT");
        assert_eq!(delivered.data.as_ref(), b"body");
    }

    #[test]
    fn disabling_post_processing_preserves_raw_archive() {
        let zip_bytes = archive(&[("nested/AFD.TXT", Some(b"body"))]);

        let delivered = post_process_archive(false, "AFD.ZIP", &zip_bytes)
            .expect("disabled post-processing should pass through");

        assert_eq!(delivered.filename, "AFD.ZIP");
        assert_eq!(delivered.data.as_ref(), zip_bytes.as_slice());
    }
}
