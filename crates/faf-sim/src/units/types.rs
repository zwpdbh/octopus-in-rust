//! Strongly-typed unit kinds, roles, categories, and recipes.
//!
//! This module defines the abstract vocabulary the optimizer uses for units:
//! factions, tech tiers, unit kinds, functional roles, UI categories, costs,
//! and build/upgrade recipes. It is deliberately separate from the raw
//! `faf-units` index so that the optimizer can reason about "a T1 engineer"
//! without caring about faction-specific blueprint ids.
//!
//! Numeric unit attributes (cost, build power, economy, storage) live in the
//! runtime boundary table owned by [`BlueprintLibrary`](super::BlueprintLibrary).
//! The blueprint ECS world stores only symbolic identity and build/upgrade
//! relationships.

use faf_units::BuildTargetStats;
use serde::{Deserialize, Serialize};

/// Faction a unit belongs to.
///
/// `Common` is used for units that exist in every faction with the same
/// abstract role, such as mass extractors, power generators, engineers, and
/// land factories. Unique units carry their actual faction tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Faction {
    Uef,
    Aeon,
    Seraphim,
    Cybran,
    Common,
}

/// Technology tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TechLevel {
    T1,
    T2,
    T3,
    T4,
}

/// Strongly-typed identifier for a unit that does not fit the common
/// economic/builder taxonomy.
///
/// Internally this is just the original uppercase blueprint id. It exists as a
/// newtype so that unique and common units are never confused at the type
/// level.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UnitId(pub String);

/// Abstract unit kind used throughout the optimizer.
///
/// Common economic and builder units are first-class variants. Everything else
/// is a `Unique` unit identified by its original blueprint id.
///
/// `UnitKind` is the **canonical identity** of a unit for recipes, queries, and
/// the optimizer. The functional role is available separately via [`role_of`]
/// and the ECS [`UnitRoleComp`](super::components::UnitRoleComp) component.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum UnitKind {
    Commander,
    Engineer(TechLevel),
    Factory(TechLevel),
    Mex(TechLevel),
    Pgen(TechLevel),
    /// T2 mass extractor surrounded by four mass storages.
    CapT2Mex,
    /// T3 mass extractor surrounded by four mass storages.
    CapT3Mex,
    EnergyStorage,
    /// T4 experimental unit (canonical UEF Fatboy representative).
    Experimental,
    Unique(UnitId),
}

/// Functional role of a unit.
///
/// This is a coarser classification than [`UnitKind`]. It groups units by what
/// they do economically (commander, builder, factory, mass extractor, etc.)
/// without encoding tech tier or faction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum UnitRole {
    Commander,
    Engineer,
    Factory,
    MassExtractor,
    PowerGenerator,
    EnergyStorage,
    CappedMassExtractor,
    Experimental,
    Other,
}

/// Extract the tech tier from a unit kind, if it has one.
pub fn tech_level_of(kind: &UnitKind) -> Option<TechLevel> {
    match kind {
        UnitKind::Engineer(t) | UnitKind::Factory(t) | UnitKind::Mex(t) | UnitKind::Pgen(t) => {
            Some(*t)
        }
        UnitKind::Experimental => Some(TechLevel::T4),
        _ => None,
    }
}

/// True if `kind` carries the given tech tier.
pub fn matches_tech_level(kind: &UnitKind, tech: TechLevel) -> bool {
    matches!(
        kind,
        UnitKind::Engineer(t)
            | UnitKind::Factory(t)
            | UnitKind::Mex(t)
            | UnitKind::Pgen(t)
            if *t == tech
    ) || matches!(kind, UnitKind::Experimental if tech == TechLevel::T4)
}

/// Derive the functional role for a unit kind.
pub fn role_of(kind: &UnitKind) -> UnitRole {
    match kind {
        UnitKind::Commander => UnitRole::Commander,
        UnitKind::Engineer(_) => UnitRole::Engineer,
        UnitKind::Factory(_) => UnitRole::Factory,
        UnitKind::Mex(_) => UnitRole::MassExtractor,
        UnitKind::Pgen(_) => UnitRole::PowerGenerator,
        UnitKind::EnergyStorage => UnitRole::EnergyStorage,
        UnitKind::CapT2Mex | UnitKind::CapT3Mex => UnitRole::CappedMassExtractor,
        UnitKind::Experimental => UnitRole::Experimental,
        UnitKind::Unique(_) => UnitRole::Other,
    }
}

/// Broad category used to group units in build palettes and summaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum UnitCategory {
    Commander,
    Engineer,
    Factory,
    Economic,
    Military,
    Other,
}

impl UnitCategory {
    /// Human-readable label for the category.
    pub fn label(self) -> &'static str {
        match self {
            UnitCategory::Commander => "Commander",
            UnitCategory::Engineer => "Engineers",
            UnitCategory::Factory => "Factories",
            UnitCategory::Economic => "Economic",
            UnitCategory::Military => "Military",
            UnitCategory::Other => "Other",
        }
    }
}

/// Derive a UI category from a functional role.
pub fn category_of_role(role: UnitRole) -> UnitCategory {
    match role {
        UnitRole::Commander => UnitCategory::Commander,
        UnitRole::Engineer => UnitCategory::Engineer,
        UnitRole::Factory => UnitCategory::Factory,
        UnitRole::MassExtractor
        | UnitRole::PowerGenerator
        | UnitRole::EnergyStorage
        | UnitRole::CappedMassExtractor => UnitCategory::Economic,
        UnitRole::Experimental => UnitCategory::Military,
        UnitRole::Other => UnitCategory::Other,
    }
}

/// Classify a unit kind into a UI category.
///
/// Common economic/builder kinds are derived from their role. Unique units are
/// bucketed as military or other based on their blueprint id prefix.
pub fn category_of(kind: &UnitKind) -> UnitCategory {
    match kind {
        UnitKind::Unique(id) => {
            // FAF blueprint ids use the second character to indicate tech/role:
            // A = air, L = land, S = structure, R = robot/bot, B = bomber, etc.
            // The first character is faction; the third is tech tier.
            let s = id.0.as_str();
            if s.len() >= 2 {
                match &s[1..2] {
                    "A" | "L" | "R" | "B" | "S"
                        if s.len() >= 3 && s[2..3].parse::<u8>().is_ok() =>
                    {
                        UnitCategory::Military
                    }
                    _ => UnitCategory::Other,
                }
            } else {
                UnitCategory::Other
            }
        }
        _ => category_of_role(role_of(kind)),
    }
}

/// Cost to build or upgrade a unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitCost {
    pub mass: f64,
    pub energy: f64,
    pub build_time: f64,
}

impl UnitCost {
    /// Convert into the raw `BuildTargetStats` shape used by the economy math.
    pub fn to_target_stats(self) -> BuildTargetStats {
        BuildTargetStats {
            build_cost_mass: self.mass,
            build_cost_energy: self.energy,
            build_time: self.build_time,
        }
    }
}

/// Symbolic rule for constructing a brand-new unit.
///
/// The target is implicit from where the rule is stored. `prereq` is the unit
/// that must already be completed before construction can start; `builders`
/// lists the legal builder kinds.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildRule {
    pub prereq: Option<UnitKind>,
    pub builders: Vec<UnitKind>,
}

/// One upgrade edge in the tech tree.
///
/// The source unit is implicit from where the path is stored. `target` is the
/// unit the source can become, and `builders` lists the legal upgrade assisters.
#[derive(Debug, Clone, PartialEq)]
pub struct UpgradePath {
    pub target: UnitKind,
    pub builders: Vec<UnitKind>,
}
