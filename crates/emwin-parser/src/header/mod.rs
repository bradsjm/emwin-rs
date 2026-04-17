//! Parse WMO headers and AFOS PIL data for text products.
//!
//! The parser module keeps transport cleanup and borrowed parsing logic together, while the
//! enrichment layer turns raw header fields into routing metadata that later stages can use
//! without reparsing the original bulletin text.

#![allow(missing_docs)]

mod enrich;
mod parser;

pub use enrich::{BbbKind, TextProductEnrichment, enrich_header};
pub(crate) use parser::{
    ParsedTextProductRef, ParsedWmoBulletinRef, condition_text_bytes,
    parse_text_product_conditioned_ref, parse_wmo_bulletin_conditioned_ref,
};
pub use parser::{ParserError, TextProductHeader, WmoHeader, parse_text_product};
