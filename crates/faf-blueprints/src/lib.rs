//! ECS-backed blueprint library and unit knowledge for FAF.
//!
//! `faf-blueprints` is the single source of truth for unit kinds, factions,
//! tech levels, build/upgrade rules, and the [`BlueprintLibrary`] that indexes
//! them. It sits on top of the raw `faf-units` parser and is used by the
//! simulator, scheduler, predictor, CLI, backend, and frontend.

pub use blueprint::BlueprintLibrary;
pub use components::{
    attributes::{
        BlueprintBundle, BlueprintId, DisplayName, FactionComp, TechLevelComp, UnitKindComp,
        UnitRoleComp,
    },
    relationships::{BuiltBy, UpgradesInto},
};
pub use graph::{BlueprintEdge, BlueprintGraph, BlueprintNode};
pub use loader::{default_units_path, load_default_data_index};
pub use types::{
    category_of, category_of_role, matches_tech_level, role_of, tech_level_of, BuildRule, Faction,
    TechLevel, UnitCategory, UnitCost, UnitId, UnitKind, UnitRole, UpgradePath,
};
pub use unit_eco::{AdjacencyBonus, UnitEcoStats};

mod blueprint;
mod build;
mod components;
mod graph;
mod loader;
mod types;
mod unit_eco;
