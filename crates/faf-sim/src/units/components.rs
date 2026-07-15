//! Bevy ECS components for blueprint entities.
//!
//! Each unit definition loaded from `faf-units` becomes a single blueprint entity
//! in [`BlueprintLibrary`](super::BlueprintLibrary). The entity carries one
//! component for each static attribute: identity, classification, cost,
//! build power, economy, storage, and recipes.
//!
//! # Wrapper components
//!
//! Domain types such as [`Faction`](super::types::Faction) and
//! [`UnitRole`](super::types::UnitRole) are deliberately **not** derived as
//! `Component` themselves. Instead, each domain type is wrapped in a dedicated
//! `*Comp` tuple struct. This keeps the domain types usable in non-ECS code
//! (recipes, classification logic, the optimizer) while making it explicit which
//! values are attached to blueprint entities.

use bevy_ecs::prelude::*;

use super::types::{Faction, TechLevel, UnitCost, UnitKind, UnitRole};

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

/// Build/upgrade cost.
#[derive(Component, Clone, Copy, Debug)]
pub struct UnitCostComp(pub UnitCost);

impl From<UnitCost> for UnitCostComp {
    fn from(value: UnitCost) -> Self {
        Self(value)
    }
}

impl From<UnitCostComp> for UnitCost {
    fn from(value: UnitCostComp) -> Self {
        value.0
    }
}

/// Build power contributed by this unit, if any.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct BuildPower(pub f64);

/// Mass/energy production and maintenance consumption.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct EconomyProfile {
    pub production_per_second_mass: f64,
    pub production_per_second_energy: f64,
    pub maintenance_consumption_per_second_energy: f64,
}

/// Mass/energy storage capacity provided by this unit.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct StorageProfile {
    pub mass: f64,
    pub energy: f64,
}

/// Functional role of the unit.
#[derive(Component, Clone, Copy, Debug)]
pub struct UnitRoleComp(pub UnitRole);

/// Recipe for constructing this unit from scratch.
#[derive(Component, Clone, Debug)]
pub struct BuildRecipeComp {
    pub prereq: Option<UnitKind>,
    pub builder_options: Vec<UnitKind>,
}

/// All upgrade recipes that start from this unit.
#[derive(Component, Clone, Debug, Default)]
pub struct UpgradeRecipesComp(pub Vec<super::types::UpgradeRecipe>);

/// Component bundle spawned for each unit blueprint.
#[derive(Bundle, Clone, Debug)]
pub struct BlueprintBundle {
    pub blueprint_id: BlueprintId,
    pub kind: UnitKindComp,
    pub role: UnitRoleComp,
    pub faction: FactionComp,
    pub display_name: DisplayName,
    pub cost: UnitCostComp,
    pub build_power: BuildPower,
    pub economy: EconomyProfile,
    pub storage: StorageProfile,
}
