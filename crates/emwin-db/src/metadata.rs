use emwin_parser::{detail_product_v2, enrich_product, summarize_product_v2};
pub use emwin_service::CompletedFileMetadata;
use emwin_service::SourceKind;

/// Builds the shared metadata DTO from raw product bytes at the persistence boundary.
pub fn build_completed_file_metadata(
    filename: &str,
    timestamp_utc: u64,
    origin: SourceKind,
    data: &[u8],
) -> CompletedFileMetadata {
    let product = enrich_product(filename, data);
    let product_summary = summarize_product_v2(&product);
    let product_detail = detail_product_v2(&product);

    CompletedFileMetadata {
        filename: filename.to_string(),
        size: data.len(),
        timestamp_utc,
        origin,
        product,
        product_summary,
        product_detail,
    }
}

#[cfg(test)]
mod tests {
    use super::build_completed_file_metadata;
    use emwin_service::SourceKind;

    #[test]
    fn serialization_preserves_existing_sidecar_shape() {
        let metadata = build_completed_file_metadata(
            "AFDBOX.TXT",
            1,
            SourceKind::Qbt,
            b"000 \nFXUS61 KBOX 022101\nAFDBOX\nBody\n",
        );

        let value = serde_json::to_value(&metadata).expect("metadata should serialize");
        assert_eq!(value["filename"], "AFDBOX.TXT");
        assert_eq!(value["size"], metadata.size);
        assert_eq!(value["timestamp_utc"], 1);
        assert_eq!(value["product"]["schema_version"], 2);
        assert!(value.get("origin").is_none());
        assert!(value.get("product_summary").is_none());
    }
}
