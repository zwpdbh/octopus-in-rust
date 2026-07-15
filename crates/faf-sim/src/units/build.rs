//! Construction helpers for building [`BlueprintLibrary`] from the raw
//! `faf-units` index.
//!
//! These functions run once during [`BlueprintLibrary::new`](super::BlueprintLibrary::new)
//! to classify raw blueprints into abstract [`UnitKind`]s and derive build
//! rules for faction-unique units. After construction the optimizer no longer
//! needs the raw index.

use faf_units::Unit;

use crate::runtime::UnitEcoStats;

use super::components::{
    attributes::{
        BlueprintBundle, BlueprintId, DisplayName, FactionComp, UnitKindComp, UnitRoleComp,
    },
    relationships::BuiltBy,
};
use super::types::{role_of, Faction, TechLevel, UnitId, UnitKind};

/// Build a symbolic blueprint component bundle from a raw `Unit`, if the unit is
/// relevant to the optimizer.
pub(crate) fn blueprint_bundle(unit: &Unit) -> Option<BlueprintBundle> {
    let kind = classify_unit(unit)?;

    Some(BlueprintBundle {
        blueprint_id: BlueprintId(unit.id.to_ascii_uppercase()),
        kind: UnitKindComp(kind.clone()),
        role: UnitRoleComp(role_of(&kind)),
        faction: FactionComp(faction_from_unit(unit)),
        display_name: DisplayName(unit.display_name()),
    })
}

/// Compute the flat runtime economic descriptor for a raw `Unit`.
///
/// This is intentionally kept separate from [`blueprint_bundle`]: the blueprint
/// entity carries only symbolic components, while numeric stats live in the
/// runtime boundary table owned by `BlueprintLibrary`.
pub(crate) fn unit_eco_stats(unit: &Unit, kind: &UnitKind) -> UnitEcoStats {
    let economy = unit.economy.as_ref();
    let target_stats = economy.and_then(|e| e.target_stats());
    let builder = economy.and_then(|e| e.builder_capability());

    let build_power = builder.map(|b| b.build_rate).unwrap_or(0.0);
    let production_per_second_mass = economy
        .and_then(|e| e.production_per_second_mass)
        .unwrap_or(0.0);
    let production_per_second_energy = economy
        .and_then(|e| e.production_per_second_energy)
        .unwrap_or(0.0);
    let maintenance_consumption_per_second_energy = economy
        .and_then(|e| e.maintenance_consumption_per_second_energy)
        .unwrap_or(0.0);
    let raw_mass_storage = economy.and_then(|e| e.storage_mass).unwrap_or(0.0);
    let raw_energy_storage = economy.and_then(|e| e.storage_energy).unwrap_or(0.0);

    let (production_mass, production_energy, maintenance, mass_storage, energy_storage) = match kind
    {
        UnitKind::Commander => (
            production_per_second_mass,
            production_per_second_energy,
            maintenance_consumption_per_second_energy,
            raw_mass_storage,
            raw_energy_storage,
        ),
        UnitKind::Engineer(_) | UnitKind::Factory(_) => (
            0.0,
            0.0,
            maintenance_consumption_per_second_energy,
            0.0,
            0.0,
        ),
        UnitKind::Mex(_) => (
            production_per_second_mass,
            0.0,
            maintenance_consumption_per_second_energy,
            0.0,
            0.0,
        ),
        UnitKind::Pgen(_) => (
            0.0,
            production_per_second_energy,
            maintenance_consumption_per_second_energy,
            0.0,
            0.0,
        ),
        UnitKind::EnergyStorage => (0.0, 0.0, 0.0, 0.0, raw_energy_storage),
        UnitKind::CapT2Mex | UnitKind::CapT3Mex => {
            // These are inserted manually in BlueprintLibrary::new, not from raw units.
            (0.0, 0.0, 0.0, 0.0, 0.0)
        }
        UnitKind::Unique(_) => (
            0.0,
            0.0,
            maintenance_consumption_per_second_energy,
            0.0,
            0.0,
        ),
    };

    UnitEcoStats {
        build_power,
        mass_cost: target_stats.map(|s| s.build_cost_mass).unwrap_or(0.0),
        energy_cost: target_stats.map(|s| s.build_cost_energy).unwrap_or(0.0),
        build_time: target_stats.map(|s| s.build_time).unwrap_or(0.0),
        production_per_second_mass: production_mass,
        production_per_second_energy: production_energy,
        maintenance_consumption_per_second_energy: maintenance,
        mass_storage,
        energy_storage,
        unit_id: Some(unit.display_name()),
    }
}

/// The default build rule for faction-unique units.
pub(crate) fn unique_unit_build_rule() -> BuiltBy {
    BuiltBy {
        prereq: None,
        builders: vec![UnitKind::Engineer(TechLevel::T3)],
    }
}

/// Map a raw unit to its abstract `UnitKind`, if any.
pub(crate) fn classify_unit(unit: &Unit) -> Option<UnitKind> {
    if unit.has_category("COMMAND") {
        return Some(UnitKind::Commander);
    }

    let tech = tech_level(unit)?;

    if unit.has_category("ENGINEER") {
        return Some(UnitKind::Engineer(tech));
    }

    if unit.has_category("FACTORY")
        && !unit.has_category("AIR")
        && !unit.has_category("NAVAL")
        && !unit.has_category("SUPPORTFACTORY")
        && !unit.has_category("GATE")
    {
        return Some(UnitKind::Factory(tech));
    }

    if unit.has_category("MASSEXTRACTION") {
        return Some(UnitKind::Mex(tech));
    }

    if unit.has_category("ENERGYPRODUCTION") && !unit.has_category("HYDROCARBON") {
        return Some(UnitKind::Pgen(tech));
    }

    if unit.has_category("ENERGYSTORAGE") {
        return Some(UnitKind::EnergyStorage);
    }

    Some(UnitKind::Unique(UnitId(unit.id.to_ascii_uppercase())))
}

/// Extract the tech level from a raw unit's categories.
pub(crate) fn tech_level(unit: &Unit) -> Option<TechLevel> {
    if unit.has_category("TECH1") {
        Some(TechLevel::T1)
    } else if unit.has_category("TECH2") {
        Some(TechLevel::T2)
    } else if unit.has_category("TECH3") {
        Some(TechLevel::T3)
    } else if unit.has_category("TECH4") || unit.has_category("EXPERIMENTAL") {
        Some(TechLevel::T4)
    } else {
        None
    }
}

/// Map a raw unit's faction metadata to a `Faction`.
pub(crate) fn faction_from_unit(unit: &Unit) -> Faction {
    match unit.faction() {
        Some("UEF") => Faction::Uef,
        Some("Aeon") => Faction::Aeon,
        Some("Seraphim") => Faction::Seraphim,
        Some("Cybran") => Faction::Cybran,
        _ => Faction::Common,
    }
}

/// True for unit kinds that are shared across factions.
pub(crate) fn is_common_kind(kind: &UnitKind) -> bool {
    !matches!(kind, UnitKind::Unique(_))
}

/// The canonical UEF blueprint id used as the representative for a common
/// `UnitKind`. Using a single faction keeps the abstract stats deterministic.
pub(crate) fn canonical_blueprint_id(kind: &UnitKind) -> Option<&'static str> {
    use TechLevel::*;
    use UnitKind::*;
    match kind {
        Commander => Some("UEL0001"),
        Engineer(T1) => Some("UEL0105"),
        Engineer(T2) => Some("UEL0208"),
        Engineer(T3) => Some("UEL0309"),
        Factory(T1) => Some("UEB0101"),
        Factory(T2) => Some("UEB0201"),
        Factory(T3) => Some("UEB0301"),
        Mex(T1) => Some("UEB1103"),
        Mex(T2) => Some("UEB1202"),
        Mex(T3) => Some("UEB1302"),
        Pgen(T1) => Some("UEB1101"),
        Pgen(T2) => Some("UEB1201"),
        Pgen(T3) => Some("UEB1301"),
        EnergyStorage => Some("UEB1105"),
        _ => None,
    }
}

/// True if `unit` is the canonical representative for `kind`.
pub(crate) fn is_canonical_for_kind(unit: &Unit, kind: &UnitKind) -> bool {
    canonical_blueprint_id(kind)
        .map(|id| unit.id.eq_ignore_ascii_case(id))
        .unwrap_or(false)
}
