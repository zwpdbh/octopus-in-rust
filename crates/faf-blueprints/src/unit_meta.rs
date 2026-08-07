use faf_units::Unit;
use serde::{Deserialize, Serialize};

/// High-level unit category derived from a unit's gameplay categories.
///
/// This is UI-only metadata: it does not affect the economy simulation, but it
/// lets the frontend group and filter units the same way `faf-db-web` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitCategory {
    Land,
    Air,
    Naval,
    StructuresFactories,
    StructuresEconomy,
    StructuresWeapons,
    StructuresSupport,
    StructuresIntelligence,
    ConstructionBuildpower,
    Experimental,
}

impl UnitCategory {
    pub fn label(self) -> &'static str {
        match self {
            UnitCategory::Land => "Land",
            UnitCategory::Air => "Air",
            UnitCategory::Naval => "Naval",
            UnitCategory::StructuresFactories => "Structures - Factories",
            UnitCategory::StructuresEconomy => "Structures - Economy",
            UnitCategory::StructuresWeapons => "Structures - Weapons",
            UnitCategory::StructuresSupport => "Structures - Support",
            UnitCategory::StructuresIntelligence => "Structures - Intelligence",
            UnitCategory::ConstructionBuildpower => "Construction - Buildpower",
            UnitCategory::Experimental => "Experimental",
        }
    }
}

/// Broad unit kind: mobile (land/air/naval) or structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitKind {
    Land,
    Air,
    Naval,
    Base,
    Unknown,
}

impl UnitKind {
    pub fn label(self) -> &'static str {
        match self {
            UnitKind::Land => "Land",
            UnitKind::Air => "Air",
            UnitKind::Naval => "Naval",
            UnitKind::Base => "Base",
            UnitKind::Unknown => "Unknown",
        }
    }
}

/// Compute the unit category for a raw FAF unit.
///
/// Mirrors the categorization used by `faf-db-web`.
pub fn classify_category(unit: &Unit) -> UnitCategory {
    if unit.has_category("ENGINEER") {
        return UnitCategory::ConstructionBuildpower;
    }
    if unit.has_category("EXPERIMENTAL") || unit.has_category("TECH4") {
        return UnitCategory::Experimental;
    }
    if unit.has_category("FACTORY") && !unit.has_category("GATE") {
        return UnitCategory::StructuresFactories;
    }
    if unit.has_category("STRUCTURE") {
        if unit.has_category("INTELLIGENCE")
            || unit.has_category("OMNI")
            || unit.has_category("RADAR")
            || unit.has_category("SONAR")
        {
            return UnitCategory::StructuresIntelligence;
        }
        if unit.has_category("ECONOMIC")
            || unit.has_category("MASSEXTRACTION")
            || unit.has_category("ENERGYPRODUCTION")
            || unit.has_category("ENERGYSTORAGE")
            || unit.has_category("MASSSTORAGE")
        {
            return UnitCategory::StructuresEconomy;
        }
        if unit.has_category("WEAPON")
            || unit.has_category("ARTILLERY")
            || unit.has_category("NUKE")
            || unit.has_category("ANTIMISSILE")
        {
            return UnitCategory::StructuresWeapons;
        }
        return UnitCategory::StructuresSupport;
    }
    if unit.has_category("AIR") {
        return UnitCategory::Air;
    }
    if unit.has_category("NAVAL") {
        return UnitCategory::Naval;
    }
    UnitCategory::Land
}

/// Compute the broad kind for a raw FAF unit.
pub fn unit_kind(unit: &Unit) -> UnitKind {
    if unit.has_category("MOBILE") {
        if unit.has_category("AIR") {
            return UnitKind::Air;
        }
        if unit.has_category("NAVAL") {
            return UnitKind::Naval;
        }
        return UnitKind::Land;
    }
    if unit.has_category("STRUCTURE") {
        return UnitKind::Base;
    }
    UnitKind::Unknown
}
