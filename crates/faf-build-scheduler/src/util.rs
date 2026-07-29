//! Shared helpers for the scheduler.
use faf_blueprints::{BlueprintLibrary, UnitKind, UnitRole};

/// Returns true if the unit kind is a mass extractor (including capped).
pub(crate) fn is_mex(library: &BlueprintLibrary, kind: &UnitKind) -> bool {
    library.role(kind) == UnitRole::MassExtractor
}

/// Counts how many mass extractors are in an iterator of unit kinds.
pub(crate) fn count_mex_from_iter<'a>(
    kinds: impl IntoIterator<Item = &'a UnitKind>,
    library: &BlueprintLibrary,
) -> u32 {
    kinds
        .into_iter()
        .filter(|kind| is_mex(library, kind))
        .count() as u32
}
