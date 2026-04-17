//! Assembly of public `ProductEnrichment` values from parsed candidates.
//!
//! Phase 2 removes parser selection from assembly. The classification stage now
//! owns all specialized parsing, and assembly performs a pure conversion from
//! candidate to the public output model.

mod base;
mod dispatch;
mod fallback;
mod specialized;

pub(crate) use dispatch::assemble_product_enrichment;

#[cfg(test)]
mod tests;
