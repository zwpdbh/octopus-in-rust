//! Unified unit knowledge repository.
//!
//! `BlueprintLibrary` is a self-contained, ECS-backed model of the units that
//! matter for build-order optimization. It is built once from the raw
//! `faf-units` index and then used without string lookups by the simulator and
//! planners.
//!
//! Each unit definition is represented as a blueprint entity in a dedicated
//! Bevy `World`. Static attributes are stored as components; see the
//! `components` module for the full list.
//!
//! The model deliberately abstracts away faction-specific names for common
//! economic and builder units. A T1 engineer is just `UnitKind::Engineer(T1)`,
//! regardless of whether the raw blueprint is `UEL0105`, `URL0105`, or
//! `UAL0105`. Faction-unique units (e.g., the Monkeylord) are represented as
//! `UnitKind::Unique(UnitId)`.
//!
//! Module layout:
//!
//! - `types` — typed unit kinds, factions, tech levels, roles, categories, costs, and recipes.
//! - `components` — Bevy ECS components for blueprint entities.
//! - `build` — helpers that classify raw blueprints and build recipes.
//! - `blueprint` — the `BlueprintLibrary` implementation.
//! - `mod` — re-exports only.

pub use blueprint::BlueprintLibrary;
pub use components::{
    BlueprintBundle, BlueprintId, BuildPower, BuildRecipeComp, DisplayName, EconomyProfile,
    FactionComp, StorageProfile, TechLevelComp, UnitCostComp, UnitKindComp, UnitRoleComp,
    UpgradeRecipesComp,
};
pub use types::{
    category_of, category_of_role, matches_tech_level, role_of, tech_level_of, BuildRecipe,
    Faction, TechLevel, UnitCategory, UnitCost, UnitId, UnitKind, UnitRole, UpgradeRecipe,
};

mod blueprint;
mod build;
mod components;
mod types;
