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

use bevy_ecs::prelude::*;

use super::super::types::{UnitKind, UpgradePath};

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

/// All upgrade destinations available from the source unit.
#[derive(Component, Clone, Debug, Default)]
pub struct UpgradesInto(pub Vec<UpgradePath>);
