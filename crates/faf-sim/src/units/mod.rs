//! Unified unit knowledge repository.
//!
//! `BlueprintLibrary` is a self-contained, ECS-backed model of the units that
//! matter for build-order optimization. It is built once from the raw
//! `faf-units` index and then used without string lookups by the simulator and
//! planners.
//!
//! Each unit definition is represented as a blueprint entity in a dedicated
//! Bevy `World`. The world stores **symbolic** components only: identity,
//! classification, and build/upgrade rules. Numeric economic attributes live in
//! the runtime boundary table owned by `BlueprintLibrary`.
//!
//! The model deliberately abstracts away faction-specific names for common
//! economic and builder units. A T1 engineer is just `UnitKind::Engineer(T1)`,
//! regardless of whether the raw blueprint is `UEL0105`, `URL0105`, or
//! `UAL0105`. Faction-unique units (e.g., the Monkeylord) are represented as
//! `UnitKind::Unique(UnitId)`.
//!
//! Module layout (mirrors `crate::runtime`):
//!
//! - `types` — typed unit kinds, factions, tech levels, roles, categories, costs, and rules.
//! - `components` — Bevy ECS components for blueprint entities.
//! - `graph` — symbolic build/upgrade graph derived from blueprint rules.
//! - `build` — helpers that classify raw blueprints.
//! - `blueprint` — the `BlueprintLibrary` implementation.
//! - `mod` — re-exports only.

pub use blueprint::BlueprintLibrary;
pub use components::{
    attributes::{
        BlueprintBundle, BlueprintId, DisplayName, FactionComp, TechLevelComp, UnitKindComp,
        UnitRoleComp,
    },
    relationships::{BuiltBy, UpgradesInto},
};
pub use graph::{BlueprintEdge, BlueprintGraph, BlueprintNode};
pub use types::{
    category_of, category_of_role, matches_tech_level, role_of, tech_level_of, BuildRule, Faction,
    TechLevel, UnitCategory, UnitCost, UnitId, UnitKind, UnitRole, UpgradePath,
};

mod blueprint;
mod build;
mod components;
mod graph;
mod types;
