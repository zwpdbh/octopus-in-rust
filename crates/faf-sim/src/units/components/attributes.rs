//! Symbolic attribute components for blueprint entities.
//!
//! These components describe identity, classification, and display metadata.
//! Numeric economic attributes (cost, build power, production, storage) are
//! intentionally **not** blueprint components; they live in the runtime boundary
//! table owned by [`BlueprintLibrary`](super::BlueprintLibrary).

use bevy_ecs::prelude::*;

use super::super::types::{Faction, TechLevel, UnitKind, UnitRole};

/// Raw blueprint identifier, e.g. `UEL0105`.
#[derive(Component, Clone, Debug)]
pub struct BlueprintId(pub String);

/// Abstract classification used by the optimizer.
#[derive(Component, Clone, Debug)]
pub struct UnitKindComp(pub UnitKind);

/// Faction the blueprint belongs to.
#[derive(Component, Clone, Copy, Debug)]
pub struct FactionComp(pub Faction);

/// Technology tier, when the unit kind has one.
#[derive(Component, Clone, Copy, Debug)]
pub struct TechLevelComp(pub TechLevel);

/// Human-readable name.
#[derive(Component, Clone, Debug)]
pub struct DisplayName(pub String);

/// Functional role of the unit.
#[derive(Component, Clone, Copy, Debug)]
pub struct UnitRoleComp(pub UnitRole);

/// Component bundle spawned for each unit blueprint.
///
/// This bundle contains only symbolic attributes. Build/upgrade rules are
/// attached separately as [`BuiltBy`](super::relationships::BuiltBy) and
/// [`UpgradesInto`](super::relationships::UpgradesInto) components.
#[derive(Bundle, Clone, Debug)]
pub struct BlueprintBundle {
    pub blueprint_id: BlueprintId,
    pub kind: UnitKindComp,
    pub role: UnitRoleComp,
    pub faction: FactionComp,
    pub display_name: DisplayName,
}
