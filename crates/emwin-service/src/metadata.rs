use crate::live::SourceKind;
use emwin_parser::{ProductDetailV2, ProductSummaryV2, detail_product_v2, summarize_product_v2};
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
}

impl CompletedFileMetadata {
    /// Builds the lightweight product projection for API and database boundaries.
    pub fn product_summary(&self) -> ProductSummaryV2 {
        summarize_product_v2(&self.product)
    }

    /// Builds the detailed product projection for sidecars and API responses.
    pub fn product_detail(&self) -> ProductDetailV2 {
        detail_product_v2(&self.product)
    }
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
        state.serialize_field("product", &self.product_detail())?;
        state.end()
    }
}
