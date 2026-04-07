use crate::live::SourceKind;
use emwin_parser::{
    ProductDetailV2, ProductEnrichment, ProductSummaryV2, detail_product_v2, enrich_product,
    summarize_product_v2,
};
use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

#[derive(Debug, Clone, PartialEq)]
pub struct CompletedFileMetadata {
    pub filename: String,
    pub size: usize,
    pub timestamp_utc: u64,
    pub origin: SourceKind,
    pub product: ProductEnrichment,
    pub product_summary: ProductSummaryV2,
    pub product_detail: ProductDetailV2,
}

impl CompletedFileMetadata {
    pub fn build(filename: &str, timestamp_utc: u64, origin: SourceKind, data: &[u8]) -> Self {
        let product = enrich_product(filename, data);
        Self {
            filename: filename.to_string(),
            size: data.len(),
            timestamp_utc,
            origin,
            product_summary: summarize_product_v2(&product),
            product_detail: detail_product_v2(&product),
            product,
        }
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
        state.serialize_field("product", &self.product_detail)?;
        state.end()
    }
}
