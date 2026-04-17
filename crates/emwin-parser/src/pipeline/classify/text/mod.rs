//! AFOS-backed classification strategies and recognition guards.

use crate::ProductEnrichmentSource;

use super::common::SupportedFamilySpec;

mod classifiers;
mod guards;
mod specs;

pub(crate) use classifiers::*;
pub(crate) use guards::*;
#[allow(unused_imports)]
pub(crate) use specs::*;
