use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::sections::Enhancement;
use crate::sections::{
    Air, Defense, Display, Economy, General, Intel, Physics, Transport, Wreckage,
};
use crate::weapon::Weapon;

/// A single FAF unit distilled from the raw blueprint.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct Unit {
    /// Blueprint id, e.g. `UEL0001`.
    #[serde(rename = "Id")]
    pub id: String,

    /// Localized description key, e.g. `<LOC uel0001_desc>Armored Command Unit`.
    #[serde(rename = "Description")]
    pub description: String,

    /// Simplified Chinese unit name, when available.
    #[serde(default, rename = "NameZh")]
    pub name_zh: Option<String>,

    /// Simplified Chinese description, when available.
    #[serde(default, rename = "DescriptionZh")]
    pub description_zh: Option<String>,

    /// Gameplay categories such as `UEF`, `TECH3`, `DIRECTFIRE`.
    #[serde(default)]
    pub categories: Vec<String>,

    /// Icon used on the strategic map.
    #[serde(default, rename = "StrategicIconName")]
    pub strategic_icon_name: Option<String>,

    /// Veterancy mass multiplier, if overridden by the blueprint.
    #[serde(default)]
    pub veteran_mass_mult: Option<f64>,

    /// Per-veterancy-level mass required to gain a veterancy level.
    #[serde(default)]
    pub veteran_mass: Option<Vec<f64>>,

    /// Split damage configuration, if the unit applies split damage logic.
    #[serde(default)]
    pub split_damage: Option<SplitDamage>,

    /// General identity information.
    #[serde(default)]
    pub general: Option<General>,

    /// Defense / shield stats.
    #[serde(default)]
    pub defense: Option<Defense>,

    /// Economy stats: costs, production, storage.
    #[serde(default)]
    pub economy: Option<Economy>,

    /// Vision, radar, sonar and stealth radii.
    #[serde(default)]
    pub intel: Option<Intel>,

    /// Movement and physics properties.
    #[serde(default)]
    pub physics: Option<Physics>,

    /// Air-specific movement stats.
    #[serde(default)]
    pub air: Option<Air>,

    /// Display information such as unit abilities.
    #[serde(default)]
    pub display: Option<Display>,

    /// Transport capacity and class restrictions.
    #[serde(default)]
    pub transport: Option<Transport>,

    /// Wreckage reclaim values.
    #[serde(default)]
    pub wreckage: Option<Wreckage>,

    /// Commander / SCU upgrades keyed by upgrade id.
    #[serde(default)]
    pub enhancements: HashMap<String, Enhancement>,

    /// Weapons equipped by the unit.
    #[serde(default)]
    pub weapon: Vec<Weapon>,
}

/// Split damage parameters.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Default)]
#[serde(rename_all = "PascalCase")]
pub struct SplitDamage {
    pub damage_amount: Option<f64>,
    pub damage_radius: Option<f64>,
}

impl Unit {
    /// Convenience accessor for the unit name when available.
    pub fn name(&self) -> Option<&str> {
        self.general.as_ref()?.unit_name.as_deref()
    }

    /// Convenience accessor for the Simplified Chinese unit name when available.
    pub fn name_zh(&self) -> Option<&str> {
        self.name_zh.as_deref()
    }

    /// Convenience accessor for the Simplified Chinese description when available.
    pub fn description_zh(&self) -> Option<&str> {
        self.description_zh.as_deref()
    }

    /// Convenience accessor for the faction name when available.
    pub fn faction(&self) -> Option<&str> {
        self.general.as_ref()?.faction_name.as_deref()
    }

    /// True if the unit belongs to the given gameplay category.
    pub fn has_category(&self, category: &str) -> bool {
        self.categories
            .iter()
            .any(|c| c.eq_ignore_ascii_case(category))
    }

    /// True if the unit belongs to the given faction.
    pub fn is_faction(&self, faction: &str) -> bool {
        self.faction()
            .map(|f| f.eq_ignore_ascii_case(faction))
            .unwrap_or(false)
    }

    /// Extracts the tech level from categories, if present.
    pub fn tech_level(&self) -> Option<&str> {
        self.categories.iter().find_map(|c| match c.as_str() {
            "TECH1" | "TECH2" | "TECH3" | "TECH4" | "EXPERIMENTAL" => Some(c.as_str()),
            _ => None,
        })
    }
}
