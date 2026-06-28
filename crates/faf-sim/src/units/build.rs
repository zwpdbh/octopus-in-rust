//! Construction helpers for building `Units` from the raw `faf-units` index.
//!
//! These functions run once during `Units::new` to classify raw blueprints into
//! abstract `UnitKind`s and derive build recipes for faction-unique units. After
//! construction the optimizer no longer needs the raw index.

use std::collections::HashMap;

use faf_units::Unit;

use crate::units::kind::{BuildRecipe, Faction, TechLevel, UnitCost, UnitDef, UnitId, UnitKind};

/// Build a `UnitDef` from a raw `Unit`, if the unit is relevant to the
/// optimizer.
pub(crate) fn unit_def(unit: &Unit) -> Option<UnitDef> {
    let kind = classify_unit(unit)?;
    let economy = unit.economy.as_ref()?;
    let target_stats = economy.target_stats()?;
    let builder = economy.builder_capability();

    Some(UnitDef {
        kind,
        faction: faction_from_unit(unit),
        display_name: unit.display_name(),
        cost: UnitCost {
            mass: target_stats.build_cost_mass,
            energy: target_stats.build_cost_energy,
            build_time: target_stats.build_time,
        },
        build_rate: builder.map(|b| b.build_rate).unwrap_or(0.0),
        mass_income: economy.production_per_second_mass.unwrap_or(0.0),
        energy_income: economy.production_per_second_energy.unwrap_or(0.0),
        maintenance_energy: economy
            .maintenance_consumption_per_second_energy
            .unwrap_or(0.0),
        mass_storage: economy.storage_mass.unwrap_or(0.0),
        energy_storage: economy.storage_energy.unwrap_or(0.0),
    })
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

    if unit.has_category("MASSSTORAGE") {
        return Some(UnitKind::MassStorage);
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

/// Derive a build recipe for a unique unit from the raw index.
///
/// This is the only place where we still inspect raw `BUILTBY*` categories.
/// It runs once during construction so that `Units` can remain self-contained.
pub(crate) fn derive_unique_recipe(
    unit: &Unit,
    _id_to_kind: &HashMap<String, UnitKind>,
) -> Option<BuildRecipe> {
    let kind = UnitKind::Unique(UnitId(unit.id.to_ascii_uppercase()));
    let mut builder_options: Vec<UnitKind> = Vec::new();

    for category in &unit.categories {
        let c = category.to_ascii_uppercase();
        match c.as_str() {
            "BUILTBYCOMMANDER" | "BUILTBYTIER1COMMANDER" => {
                push_unique(&mut builder_options, UnitKind::Commander);
            }
            "BUILTBYTIER1ENGINEER" => {
                push_unique(&mut builder_options, UnitKind::Engineer(TechLevel::T1))
            }
            "BUILTBYTIER2ENGINEER" => {
                push_unique(&mut builder_options, UnitKind::Engineer(TechLevel::T2))
            }
            "BUILTBYTIER3ENGINEER" => {
                push_unique(&mut builder_options, UnitKind::Engineer(TechLevel::T3))
            }
            "BUILTBYTIER1FACTORY" => {
                push_unique(&mut builder_options, UnitKind::Factory(TechLevel::T1))
            }
            "BUILTBYTIER2FACTORY" => {
                push_unique(&mut builder_options, UnitKind::Factory(TechLevel::T2))
            }
            "BUILTBYTIER3FACTORY" => {
                push_unique(&mut builder_options, UnitKind::Factory(TechLevel::T3))
            }
            _ => {}
        }
    }

    if builder_options.is_empty() {
        return None;
    }

    let prereq = tech_level(unit).and_then(|tech| match tech {
        TechLevel::T1 => None,
        TechLevel::T2 => Some(UnitKind::Factory(TechLevel::T1)),
        TechLevel::T3 | TechLevel::T4 => Some(UnitKind::Factory(TechLevel::T3)),
    });

    Some(BuildRecipe {
        target: kind,
        prereq,
        builder_options,
    })
}

pub(crate) fn push_unique(vec: &mut Vec<UnitKind>, kind: UnitKind) {
    if !vec.contains(&kind) {
        vec.push(kind);
    }
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
        MassStorage => Some("UEB1106"),
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
