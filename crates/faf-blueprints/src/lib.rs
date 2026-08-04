//! ECS-backed blueprint library and unit knowledge for FAF.
//!
//! `faf-blueprints` is the single source of truth for unit kinds, factions,
//! tech levels, build/upgrade rules, and the [`BlueprintLibrary`] that indexes
//! them. It sits on top of the raw `faf-units` parser and is used by the
//! simulator, scheduler, predictor, CLI, backend, and frontend.

pub use blueprint::FAFBlueprint;
pub use components::{
    attributes::{
        BlueprintBundle, BlueprintId, DisplayName, FactionComp, TechLevelComp, UnitKindComp,
        UnitRoleComp,
    },
    relationships::{BuiltBy, CapsInto, UpgradesInto},
};
pub use graph::{BlueprintEdge, BlueprintGraph, BlueprintNode};
pub use types::{
    category_of, category_of_role, matches_tech_level, role_of, tech_level_of, BuildRule, Faction,
    TechLevel, UnitCategory, UnitCost, UnitId, UnitKind, UnitRole,
};
pub use unit_eco::{AdjacencyBonus, UnitEcoStats};

mod blueprint;
mod build;
mod components;
mod graph;
mod types;
mod unit_eco;
