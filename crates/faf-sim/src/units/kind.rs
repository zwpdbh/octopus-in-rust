//! Strongly-typed unit kinds and definitions.
//!
//! This module defines the abstract vocabulary the optimizer uses for units:
//! factions, tech tiers, unit kinds, costs, and build/upgrade recipes. It is
//! deliberately separate from the raw `faf-units` index so that the optimizer
//! can reason about "a T1 engineer" without caring about faction-specific
//! blueprint ids.

use faf_units::BuildTargetStats;

use crate::economy::{EcoConsumer, EcoFlow, EcoProducer};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UnitId(pub String);

/// Abstract unit kind used throughout the optimizer.
///
/// Common economic and builder units are first-class variants. Everything else
/// is a `Unique` unit identified by its original blueprint id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum UnitKind {
    Commander,
    Engineer(TechLevel),
    Factory(TechLevel),
    Mex(TechLevel),
    Pgen(TechLevel),
    Unique(UnitId),
}

/// Cost to build or upgrade a unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitCost {
    pub mass: f64,
    pub energy: f64,
    pub build_time: f64,
}

/// Static definition of a unit.
///
/// This is the optimizer's view of a unit: all stats needed for economy and
/// build-power calculations, plus metadata for display and faction scoping.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitDef {
    pub kind: UnitKind,
    pub faction: Faction,
    pub display_name: String,
    pub cost: UnitCost,
    pub build_rate: f64,
    pub mass_income: f64,
    pub energy_income: f64,
    pub maintenance_energy: f64,
    pub mass_storage: f64,
    pub energy_storage: f64,
}

/// Recipe for constructing a brand-new unit.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildRecipe {
    pub target: UnitKind,
    /// The unit that must already be completed before this action is legal.
    /// `None` means no prerequisite (e.g., the commander or a T1 factory built
    /// straight from the ACU).
    pub prereq: Option<UnitKind>,
    /// Any of these builder kinds is a legal choice for the action.
    pub builder_options: Vec<UnitKind>,
}

/// Recipe for upgrading an existing unit in-place.
#[derive(Debug, Clone, PartialEq)]
pub struct UpgradeRecipe {
    pub from: UnitKind,
    pub to: UnitKind,
    pub cost: UnitCost,
    /// Any of these builder kinds can assist the upgrade.
    pub builder_options: Vec<UnitKind>,
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

impl EcoProducer for UnitDef {
    fn production(&self) -> EcoFlow {
        EcoFlow {
            mass_per_second: self.mass_income,
            energy_per_second: self.energy_income,
        }
    }
}

impl EcoConsumer for UnitDef {
    fn consumption(&self) -> EcoFlow {
        EcoFlow {
            mass_per_second: 0.0,
            energy_per_second: self.maintenance_energy,
        }
    }
}
