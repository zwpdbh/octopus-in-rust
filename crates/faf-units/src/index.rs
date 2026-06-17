use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::Unit;

/// Root wrapper for the generated FAF unit index.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DataIndex {
    pub version: String,

    pub shield_default_overspill: f64,
    pub shield_default_recharge_time: f64,

    pub tech_to_vet_multipliers: HashMap<String, f64>,

    /// Per-tech veterancy regen buffs, indexed [tech_index][vet_level].
    pub veterancy_regen_buffs: Vec<Vec<f64>>,

    pub wreckage_tech_mass_mults: HashMap<String, f64>,
    pub wreckage_water_mult: f64,

    #[serde(default)]
    pub units: Vec<Unit>,
}

impl DataIndex {
    /// Look up a unit by blueprint id (case-insensitive).
    pub fn find_unit(&self, id: &str) -> Option<&Unit> {
        self.units.iter().find(|u| u.id.eq_ignore_ascii_case(id))
    }

    /// Units matching all of the given categories.
    pub fn units_with_categories<'a>(
        &'a self,
        categories: &'a [&str],
    ) -> impl Iterator<Item = &'a Unit> + 'a {
        self.units
            .iter()
            .filter(move |u| categories.iter().all(|c| u.has_category(c)))
    }

    /// Units whose name or id contains the query (case-insensitive).
    pub fn search<'a>(&'a self, query: &'a str) -> impl Iterator<Item = &'a Unit> + 'a {
        let query = query.to_lowercase();
        self.units.iter().filter(move |u| {
            u.id.to_lowercase().contains(&query)
                || u.description.to_lowercase().contains(&query)
                || u.name()
                    .map(|n| n.to_lowercase().contains(&query))
                    .unwrap_or(false)
        })
    }
}
