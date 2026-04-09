use crate::live::SourceKind;
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

/// Shared metadata payload carried across live, persistence, and HTTP boundaries.
#[derive(Debug, Clone, PartialEq)]
pub struct CompletedFileMetadata {
    pub filename: String,
    pub size: usize,
    pub timestamp_utc: u64,
    pub origin: SourceKind,
    pub product: emwin_parser::ProductEnrichment,
    pub product_summary: emwin_parser::ProductSummaryV2,
    pub product_detail: emwin_parser::ProductDetailV2,
}

impl Serialize for CompletedFileMetadata {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("CompletedFileMetadata", 4)?;
        state.serialize_field("filename", &self.filename)?;
        state.serialize_field("size", &self.size)?;
        state.serialize_field("timestamp_utc", &self.timestamp_utc)?;
        state.serialize_field("product", &self.product_detail)?;
        state.end()
    }
}
