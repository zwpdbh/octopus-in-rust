//! Explicit upgrade cost table for FAF build-order planning.
//!
//! The upstream unit data JSON does not contain upgrade costs or relationships,
//! so this module provides a small, hand-curated table keyed by the source unit
//! id. Each entry records the single unit it upgrades into and the cost of that
//! upgrade. FAF upgrade chains are linear: a given unit upgrades to at most one
//! other unit.

use std::collections::HashMap;

use faf_units::{BuildTargetStats, DataIndex, Unit};

/// Resources and work required for a single upgrade step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UpgradeCost {
    /// Mass required to start and complete the upgrade.
    pub mass: f64,
    /// Energy required to start and complete the upgrade.
    pub energy: f64,
    /// Build-time work required to complete the upgrade.
    pub build_time: f64,
}

impl UpgradeCost {
    /// Convert this upgrade cost into the same shape used for normal build
    /// targets so the simulator can consume it uniformly.
    pub fn to_build_target_stats(&self) -> BuildTargetStats {
        BuildTargetStats {
            build_cost_mass: self.mass,
            build_cost_energy: self.energy,
            build_time: self.build_time,
        }
    }
}

/// Table of upgrade costs keyed by the unit id being upgraded.
///
/// Each source unit maps to at most one upgrade target. This matches FAF, where
/// a T1 mass extractor upgrades into a T2 mass extractor, which in turn upgrades
/// into a T3 mass extractor, and so on.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UpgradeTable {
    entries: HashMap<String, (String, UpgradeCost)>,
}

impl UpgradeTable {
    /// Create an empty table.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Register an upgrade from `from_unit_id` to `to_unit_id` with the given
    /// cost.
    pub fn insert(&mut self, from_unit_id: &str, to_unit_id: &str, cost: UpgradeCost) {
        self.entries.insert(
            from_unit_id.to_ascii_uppercase(),
            (to_unit_id.to_ascii_uppercase(), cost),
        );
    }

    /// Look up the direct upgrade target and cost for `unit`.
    pub fn find(&self, unit: &Unit) -> Option<(&str, UpgradeCost)> {
        self.entries
            .get(&unit.id.to_ascii_uppercase())
            .map(|(to_id, cost)| (to_id.as_str(), *cost))
    }

    /// Look up the target unit blueprint and cost for upgrading `unit`.
    pub fn find_target(&self, unit: &Unit, index: &DataIndex) -> Option<(Unit, UpgradeCost)> {
        let (to_id, cost) = self.find(unit)?;
        let target = index.find_unit(to_id)?;
        Some((target.clone(), cost))
    }
}

/// A minimal default upgrade table for standard FAF.
///
/// Currently only mass extractor upgrades are included. This keeps the planner
/// search space under control while the upgrade action is being integrated.
/// Costs are approximate and can be tuned as needed.
pub fn default_upgrade_table() -> UpgradeTable {
    use UpgradeCost as C;

    let mut table = UpgradeTable::new();

    // UEF mex upgrades.
    table.insert(
        "UEB1103",
        "UEB1202",
        C {
            mass: 900.0,
            energy: 5400.0,
            build_time: 900.0,
        },
    );
    table.insert(
        "UEB1202",
        "UEB1302",
        C {
            mass: 4600.0,
            energy: 31625.0,
            build_time: 6000.0,
        },
    );

    // Cybran mex upgrades.
    table.insert(
        "URB1103",
        "URB1202",
        C {
            mass: 900.0,
            energy: 5400.0,
            build_time: 900.0,
        },
    );
    table.insert(
        "URB1202",
        "URB1302",
        C {
            mass: 4600.0,
            energy: 31625.0,
            build_time: 6000.0,
        },
    );

    // Aeon mex upgrades.
    table.insert(
        "UAB1103",
        "UAB1202",
        C {
            mass: 900.0,
            energy: 5400.0,
            build_time: 900.0,
        },
    );
    table.insert(
        "UAB1202",
        "UAB1302",
        C {
            mass: 4600.0,
            energy: 31625.0,
            build_time: 6000.0,
        },
    );

    // Seraphim mex upgrades.
    table.insert(
        "XSB1103",
        "XSB1202",
        C {
            mass: 900.0,
            energy: 5400.0,
            build_time: 900.0,
        },
    );
    table.insert(
        "XSB1202",
        "XSB1302",
        C {
            mass: 4600.0,
            energy: 31625.0,
            build_time: 6000.0,
        },
    );

    // Nomads mex upgrades.
    table.insert(
        "XNB1103",
        "XNB1202",
        C {
            mass: 900.0,
            energy: 5400.0,
            build_time: 900.0,
        },
    );
    table.insert(
        "XNB1202",
        "XNB1302",
        C {
            mass: 4600.0,
            energy: 31625.0,
            build_time: 6000.0,
        },
    );

    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_index() -> DataIndex {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        serde_json::from_str(json).expect("embedded index should parse")
    }

    #[test]
    fn default_table_finds_mex_upgrade() {
        let index = load_index();
        let table = default_upgrade_table();
        let t1_mex = index.find_unit("URB1103").expect("T1 mex exists");

        let (to_id, cost) = table.find(t1_mex).expect("T1->T2 mex upgrade exists");
        assert_eq!(to_id, "URB1202");
        assert!(cost.mass > 0.0);
        assert!(cost.energy > 0.0);
        assert!(cost.build_time > 0.0);
    }

    #[test]
    fn default_table_resolves_upgrade_target() {
        let index = load_index();
        let table = default_upgrade_table();
        let t1_mex = index.find_unit("URB1103").expect("T1 mex exists");

        let (target, cost) = table
            .find_target(t1_mex, &index)
            .expect("T1->T2 mex target exists");
        assert_eq!(target.id, "URB1202");
        assert_eq!(cost, table.find(t1_mex).unwrap().1);
    }

    #[test]
    fn non_upgradeable_unit_has_no_upgrades() {
        let index = load_index();
        let table = default_upgrade_table();
        let eng = index.find_unit("URL0105").expect("T1 engineer exists");

        assert!(table.find(eng).is_none());
        assert!(table.find_target(eng, &index).is_none());
    }
}
