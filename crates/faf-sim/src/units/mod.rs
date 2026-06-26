//! Unified unit knowledge repository.
//!
//! `Units` layers game-specific derived knowledge on top of the raw
//! `faf-units` index. It is the single interface the simulator and planners
//! use to ask questions about units:
//!
//! - What are the stats of a unit?
//! - Which builders can build a given target?
//! - What is the prerequisite chain to a goal unit?
//! - Can a unit be upgraded, and if so, into what and for what cost?
//!
//! The `Capability`, `TechGraph`, `UpgradeCost`, and `UpgradeTable` types are
//! re-exported from here for convenience, but most callers should only need
//! `Units`.

use std::sync::Arc;

pub use faf_units::{BuildTargetStats, DataIndex, Unit};

use crate::economy::{EcoConsumer, EcoFlow, EcoProducer};

mod tech_graph;
mod upgrade_table;

pub use tech_graph::{Capability, TechGraph, TechGraphError, TechNode};
pub use upgrade_table::{default_upgrade_table, UpgradeCost, UpgradeTable};

/// Unified repository of unit knowledge.
///
/// Owns a copy of the raw unit index plus derived data structures built from
/// it. Because it owns the index, `Units` can be stored directly in actors and
/// passed around without lifetime constraints.
#[derive(Debug, Clone)]
pub struct Units {
    index: Arc<DataIndex>,
    tech_graph: TechGraph,
    upgrade_table: UpgradeTable,
}

impl Units {
    /// Build the repository from a raw unit index.
    pub fn new(index: DataIndex) -> Self {
        let index = Arc::new(index);
        Self {
            tech_graph: TechGraph::new(Arc::clone(&index)),
            index,
            upgrade_table: default_upgrade_table(),
        }
    }

    /// Build the repository from a borrowed raw unit index.
    pub fn from_ref(index: &DataIndex) -> Self {
        Self::new(index.clone())
    }

    /// Build the repository with a custom upgrade table.
    pub fn with_upgrade_table(index: DataIndex, upgrade_table: UpgradeTable) -> Self {
        let index = Arc::new(index);
        Self {
            tech_graph: TechGraph::new(Arc::clone(&index)),
            index,
            upgrade_table,
        }
    }

    /// Access the underlying raw unit index.
    pub fn index(&self) -> &DataIndex {
        &self.index
    }

    /// Access the derived capability graph.
    pub fn tech_graph(&self) -> &TechGraph {
        &self.tech_graph
    }

    /// Access the upgrade cost table.
    pub fn upgrade_table(&self) -> &UpgradeTable {
        &self.upgrade_table
    }

    /// Look up a unit by blueprint id.
    pub fn find(&self, id: &str) -> Option<&Unit> {
        self.index.find_unit(id)
    }

    /// Iterate over every unit in the raw index.
    pub fn all_units(&self) -> &[Unit] {
        &self.index.units
    }

    /// Build target stats for a unit, if it can be built at all.
    pub fn build_cost(&self, id: &str) -> Option<BuildTargetStats> {
        self.find(id)?.build_target_stats()
    }

    /// True if `builder_id` can build `target_id` in the capability model.
    pub fn can_build(&self, builder_id: &str, target_id: &str) -> bool {
        self.tech_graph
            .can_build(builder_id, target_id)
            .unwrap_or(false)
    }

    /// Return every unit that can build `target_id`.
    pub fn builders_for(&self, target_id: &str) -> Result<Vec<&Unit>, TechGraphError> {
        self.tech_graph.builders_for(target_id)
    }

    /// Return the prerequisite capabilities for `goal_unit_id`, stopping
    /// expansion at `start`.
    pub fn prerequisites(
        &self,
        goal_unit_id: &str,
        start: Capability,
    ) -> Result<Vec<Capability>, TechGraphError> {
        self.tech_graph.prerequisites(goal_unit_id, start)
    }

    /// Return a concrete build chain from `start` capability to `goal_unit_id`.
    pub fn prerequisite_chain(
        &self,
        goal_unit_id: &str,
        start: Capability,
    ) -> Result<Vec<(Capability, String)>, TechGraphError> {
        self.tech_graph.prerequisite_chain(goal_unit_id, start)
    }

    /// Return all prerequisite unit blueprints stopping expansion at the given
    /// unit ids.
    pub fn all_prerequisites<'b>(
        &self,
        goal_unit_id: &str,
        stop_at: &'b [&'b str],
    ) -> Result<Vec<&Unit>, TechGraphError> {
        self.tech_graph.all_prerequisites(goal_unit_id, stop_at)
    }

    /// Return all prerequisite unit blueprints using the default ACU start
    /// capability.
    pub fn all_prerequisites_default(
        &self,
        goal_unit_id: &str,
    ) -> Result<Vec<&Unit>, TechGraphError> {
        self.tech_graph.all_prerequisites_default(goal_unit_id)
    }

    /// True if this unit has a registered upgrade target.
    pub fn is_upgradeable(&self, unit_id: &str) -> bool {
        self.find(unit_id)
            .and_then(|u| self.upgrade_table.find(u))
            .is_some()
    }

    /// Return the unit this unit upgrades into, plus the upgrade cost.
    pub fn upgrade_target(&self, unit_id: &str) -> Option<(&Unit, UpgradeCost)> {
        let unit = self.find(unit_id)?;
        let (to_id, cost) = self.upgrade_table.find(unit)?;
        let target = self.find(to_id)?;
        Some((target, cost))
    }
}

impl EcoProducer for Unit {
    fn production(&self) -> EcoFlow {
        self.economy.as_ref().map_or(EcoFlow::ZERO, |e| EcoFlow {
            mass_per_second: e.production_per_second_mass.unwrap_or(0.0),
            energy_per_second: e.production_per_second_energy.unwrap_or(0.0),
        })
    }
}

impl EcoConsumer for Unit {
    fn consumption(&self) -> EcoFlow {
        self.economy.as_ref().map_or(EcoFlow::ZERO, |e| EcoFlow {
            mass_per_second: 0.0,
            energy_per_second: e.maintenance_consumption_per_second_energy.unwrap_or(0.0),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_index() -> DataIndex {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        serde_json::from_str(json).expect("embedded index should parse")
    }

    #[test]
    fn units_answers_build_and_upgrade_questions() {
        let index = load_index();
        let units = Units::new(index);

        // Build questions.
        assert!(units.can_build("URL0001", "URB1101"));
        assert!(!units.can_build("URL0001", "URL0402"));

        // Upgrade questions.
        assert!(units.is_upgradeable("URB1103"));
        let (target, cost) = units.upgrade_target("URB1103").expect("T1 mex upgrades");
        assert_eq!(target.id, "URB1202");
        assert!(cost.mass > 0.0);

        // Non-upgradeable unit.
        assert!(!units.is_upgradeable("URL0105"));
        assert!(units.upgrade_target("URL0105").is_none());
    }
}
