//! Strongly-typed unit kinds and definitions.
//!
//! This module defines the abstract vocabulary the optimizer uses for units:
//! factions, tech tiers, unit kinds, costs, and build/upgrade recipes. It is
//! deliberately separate from the raw `faf-units` index so that the optimizer
//! can reason about "a T1 engineer" without caring about faction-specific
//! blueprint ids.

use faf_units::BuildTargetStats;
use serde::{Deserialize, Serialize};

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
    Unique(UnitId),
}

/// Cost to build or upgrade a unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitCost {
    pub mass: f64,
    pub energy: f64,
    pub build_time: f64,
}

/// Functional role of a unit.
///
/// Each variant carries only the stats that are meaningful for that role. This
/// makes invalid combinations (e.g., a factory with mass income) unrepresentable
/// at the type level.
#[derive(Debug, Clone, PartialEq)]
pub enum UnitRole {
    /// Armored Command Unit: builds, produces a small base income, and provides
    /// starting storage.
    Commander {
        build_rate: f64,
        mass_income: f64,
        energy_income: f64,
        maintenance_energy: f64,
        mass_storage: f64,
        energy_storage: f64,
    },
    /// Land factory: builds units, consumes energy for maintenance.
    Factory {
        build_rate: f64,
        maintenance_energy: f64,
    },
    /// Engineer: builds units, consumes energy for maintenance.
    Engineer {
        build_rate: f64,
        maintenance_energy: f64,
    },
    /// Mass extractor: produces mass, consumes energy for maintenance.
    MassExtractor {
        mass_income: f64,
        maintenance_energy: f64,
    },
    /// Power generator: produces energy, consumes energy for maintenance.
    PowerGenerator {
        energy_income: f64,
        maintenance_energy: f64,
    },
    /// Energy storage building.
    EnergyStorage { energy_storage: f64 },
    /// T2/T3 mass extractor surrounded by four mass storages.
    CappedMassExtractor {
        mass_income: f64,
        mass_storage: f64,
        maintenance_energy: f64,
    },
    /// Any other unit (typically military/unique) with only maintenance cost.
    Other { maintenance_energy: f64 },
}

impl UnitRole {
    /// Build power contributed by this role, if any.
    pub fn build_rate(&self) -> f64 {
        match self {
            UnitRole::Commander { build_rate, .. }
            | UnitRole::Factory { build_rate, .. }
            | UnitRole::Engineer { build_rate, .. } => *build_rate,
            _ => 0.0,
        }
    }

    /// Mass income produced by this role, if any.
    pub fn mass_income(&self) -> f64 {
        match self {
            UnitRole::Commander { mass_income, .. }
            | UnitRole::MassExtractor { mass_income, .. }
            | UnitRole::CappedMassExtractor { mass_income, .. } => *mass_income,
            _ => 0.0,
        }
    }

    /// Energy income produced by this role, if any.
    pub fn energy_income(&self) -> f64 {
        match self {
            UnitRole::Commander { energy_income, .. }
            | UnitRole::PowerGenerator { energy_income, .. } => *energy_income,
            _ => 0.0,
        }
    }

    /// Energy consumed per second for maintenance, if any.
    pub fn maintenance_energy(&self) -> f64 {
        match self {
            UnitRole::Commander {
                maintenance_energy, ..
            }
            | UnitRole::Factory {
                maintenance_energy, ..
            }
            | UnitRole::Engineer {
                maintenance_energy, ..
            }
            | UnitRole::MassExtractor {
                maintenance_energy, ..
            }
            | UnitRole::PowerGenerator {
                maintenance_energy, ..
            }
            | UnitRole::CappedMassExtractor {
                maintenance_energy, ..
            }
            | UnitRole::Other { maintenance_energy } => *maintenance_energy,
            _ => 0.0,
        }
    }

    /// Mass storage capacity provided by this role, if any.
    pub fn mass_storage(&self) -> f64 {
        match self {
            UnitRole::Commander { mass_storage, .. }
            | UnitRole::CappedMassExtractor { mass_storage, .. } => *mass_storage,
            _ => 0.0,
        }
    }

    /// Energy storage capacity provided by this role, if any.
    pub fn energy_storage(&self) -> f64 {
        match self {
            UnitRole::Commander { energy_storage, .. }
            | UnitRole::EnergyStorage { energy_storage } => *energy_storage,
            _ => 0.0,
        }
    }
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
    pub role: UnitRole,
}

impl UnitDef {
    /// Build power contributed by this unit, if any.
    pub fn build_rate(&self) -> f64 {
        self.role.build_rate()
    }

    /// Mass income produced by this unit, if any.
    pub fn mass_income(&self) -> f64 {
        self.role.mass_income()
    }

    /// Energy income produced by this unit, if any.
    pub fn energy_income(&self) -> f64 {
        self.role.energy_income()
    }

    /// Energy consumed per second for maintenance, if any.
    pub fn maintenance_energy(&self) -> f64 {
        self.role.maintenance_energy()
    }

    /// Mass storage capacity provided by this unit, if any.
    pub fn mass_storage(&self) -> f64 {
        self.role.mass_storage()
    }

    /// Energy storage capacity provided by this unit, if any.
    pub fn energy_storage(&self) -> f64 {
        self.role.energy_storage()
    }
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
            mass_per_second: crate::quantities::MassRate::from_raw(self.mass_income()),
            energy_per_second: crate::quantities::EnergyRate::from_raw(self.energy_income()),
        }
    }
}

impl EcoConsumer for UnitDef {
    fn consumption(&self) -> EcoFlow {
        EcoFlow {
            mass_per_second: crate::quantities::MassRate::zero(),
            energy_per_second: crate::quantities::EnergyRate::from_raw(self.maintenance_energy()),
        }
    }
}
