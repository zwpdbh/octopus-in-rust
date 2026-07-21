//! Relationship components for blueprint entities.
//!
//! These components encode the symbolic build/upgrade graph used by the planner
//! and by rendering code. They answer questions like:
//!
//! - "What builders can construct this unit?"
//! - "What unit must already exist before this unit can be built?"
//! - "What can this unit upgrade into?"
//!
//! Numeric costs are intentionally absent here; they are resolved from the
//! target blueprint's runtime economic stats.
//!
//! Upgrades are stored as bare target identities because, in FAF, a structure
//! upgrades itself using its own `BuildRate`. Engineers or the ACU may assist,
//! but they are not required to start or complete the upgrade.

use bevy_ecs::prelude::*;

use super::super::types::UnitKind;

/// A build rule attached to the **target** unit.
///
/// `prereq` is the unit that must already be finished before construction can
/// start. `builders` lists the unit kinds that are legal builders. The target
/// itself is implicit from the entity's [`UnitKindComp`](super::attributes::UnitKindComp).
#[derive(Component, Clone, Debug)]
pub struct BuiltBy {
    pub prereq: Option<UnitKind>,
    pub builders: Vec<UnitKind>,
}

/// Attached to a source blueprint entity to indicate what it can upgrade into.
///
/// For example, the `Mex(T2)` blueprint entity carries `UpgradesInto(Mex(T3))`
/// because a T2 mass extractor can be upgraded into a T3 mass extractor. The
/// source unit itself provides the build power for the upgrade, so the component
/// stores only the target unit kind.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct UpgradesInto(pub UnitKind);

/// Attached to a source blueprint entity to indicate what it can be capped into.
///
/// In FAF a mass extractor can be surrounded by mass storages to become a
/// "capped" variant with higher income and storage. For example, the `Mex(T2)`
/// blueprint entity carries `CapsInto(CapMex(T2))` because a T2 mass extractor
/// can be capped into `CapMex(T2)`. This relationship is kept separate from
/// regular tier upgrades because it is a distinct transformation with its own
/// cost and stats.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct CapsInto(pub UnitKind);
