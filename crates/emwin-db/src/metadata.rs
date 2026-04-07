pub use emwin_service::CompletedFileMetadata;

#[cfg(test)]
mod tests {
    use super::CompletedFileMetadata;
    use emwin_service::SourceKind;

    #[test]
    fn serialization_preserves_existing_sidecar_shape() {
        let metadata = CompletedFileMetadata::build(
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
