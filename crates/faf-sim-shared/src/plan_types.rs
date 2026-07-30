//! Shared plan types that bridge the simulator and UI/tooling.

use faf_blueprints::UnitEcoStats;

use crate::{BuildQueue, BuildTask, GameEcoMetrics};
use serde::{Deserialize, Serialize};

/// Frontend-facing unit descriptor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnitSummary {
    pub id: String,
    pub display_name: String,
    pub faction: String,
    pub tech: String,
    pub category: String,
    #[serde(default)]
    pub strategic_icon_name: Option<String>,
    pub kind: String,
    #[serde(default)]
    pub build_rate: Option<f64>,
    #[serde(default)]
    pub build_cost_mass: Option<f64>,
    #[serde(default)]
    pub build_cost_energy: Option<f64>,
    #[serde(default)]
    pub build_time: Option<f64>,
    #[serde(default)]
    pub production_per_second_mass: Option<f64>,
    #[serde(default)]
    pub production_per_second_energy: Option<f64>,
    #[serde(default)]
    pub maintenance_consumption_per_second_energy: Option<f64>,
    #[serde(default)]
    pub mass_storage: Option<f64>,
    #[serde(default)]
    pub energy_storage: Option<f64>,
}

impl UnitSummary {
    /// Build a minimal builder summary from builder-only runtime stats.
    pub fn from_builder_stats(stats: &UnitEcoStats) -> Self {
        Self {
            id: stats.unit_id.clone().unwrap_or_default(),
            display_name: String::new(),
            faction: String::new(),
            tech: String::new(),
            category: String::new(),
            strategic_icon_name: None,
            kind: String::new(),
            build_rate: Some(stats.build_power),
            build_cost_mass: None,
            build_cost_energy: None,
            build_time: None,
            production_per_second_mass: None,
            production_per_second_energy: None,
            maintenance_consumption_per_second_energy: None,
            mass_storage: None,
            energy_storage: None,
        }
    }

    /// Build a minimal target summary from target-only runtime stats.
    pub fn from_target_stats(stats: &UnitEcoStats) -> Self {
        Self {
            id: stats.unit_id.clone().unwrap_or_default(),
            display_name: String::new(),
            faction: String::new(),
            tech: String::new(),
            category: String::new(),
            strategic_icon_name: None,
            kind: String::new(),
            build_rate: Some(stats.build_power),
            build_cost_mass: Some(stats.mass_cost),
            build_cost_energy: Some(stats.energy_cost),
            build_time: Some(stats.build_time),
            production_per_second_mass: Some(stats.production_per_second_mass),
            production_per_second_energy: Some(stats.production_per_second_energy),
            maintenance_consumption_per_second_energy: Some(
                stats.maintenance_consumption_per_second_energy,
            ),
            mass_storage: Some(stats.mass_storage),
            energy_storage: Some(stats.energy_storage),
        }
    }
}

/// One item in a construction plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConstructionItem {
    pub id: u32,
    pub builders: Vec<UnitSummary>,
    pub targets: Vec<UnitSummary>,
    pub start_after: usize,
}

impl ConstructionItem {
    pub fn is_valid(&self) -> bool {
        !self.builders.is_empty() && !self.targets.is_empty()
    }
}

/// Human-editable construction plan.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ConstructionPlan {
    #[serde(rename = "initial_eco", alias = "eco")]
    pub eco: GameEcoMetrics,
    pub items: Vec<ConstructionItem>,
}

impl ConstructionPlan {
    /// Convert the plan into the simulation queue format.
    pub fn to_build_queue(&self) -> BuildQueue {
        let tasks = self
            .items
            .iter()
            .map(|item| BuildTask {
                id: item.id,
                start_after: item.start_after,
                builders: item
                    .builders
                    .iter()
                    .map(|u| UnitEcoStats {
                        build_power: u.build_rate.unwrap_or(0.0),
                        mass_cost: 0.0,
                        energy_cost: 0.0,
                        build_time: 0.0,
                        unit_id: Some(u.id.clone()),
                        ..Default::default()
                    })
                    .collect(),
                targets: item
                    .targets
                    .iter()
                    .map(|t| UnitEcoStats {
                        build_power: 0.0,
                        mass_cost: t.build_cost_mass.unwrap_or(0.0),
                        energy_cost: t.build_cost_energy.unwrap_or(0.0),
                        build_time: t.build_time.unwrap_or(0.0),
                        production_per_second_mass: t.production_per_second_mass.unwrap_or(0.0),
                        production_per_second_energy: t.production_per_second_energy.unwrap_or(0.0),
                        maintenance_consumption_per_second_energy: t
                            .maintenance_consumption_per_second_energy
                            .unwrap_or(0.0),
                        mass_storage: t.mass_storage.unwrap_or(0.0),
                        energy_storage: t.energy_storage.unwrap_or(0.0),
                        unit_id: Some(t.id.clone()),
                        ..Default::default()
                    })
                    .collect(),
            })
            .collect();

        BuildQueue {
            initial_eco: self.eco,
            tasks,
        }
    }

    /// Convert a simulation queue into a plan, using the provided unit list to
    /// resolve blueprint identities.
    pub fn from_build_queue_with_units(queue: BuildQueue, units: &[UnitSummary]) -> Self {
        let unit_map: std::collections::HashMap<&str, &UnitSummary> =
            units.iter().map(|u| (u.id.as_str(), u)).collect();

        let find_unit = |r: &UnitEcoStats| -> UnitSummary {
            if let Some(id) = r.unit_id.as_deref() {
                if let Some(u) = unit_map.get(id) {
                    return (*u).clone();
                }
            }
            units
                .iter()
                .find(|u| {
                    u.build_rate.unwrap_or(0.0) == r.build_power
                        && u.build_cost_mass.unwrap_or(0.0) == r.mass_cost
                        && u.build_cost_energy.unwrap_or(0.0) == r.energy_cost
                        && u.build_time.unwrap_or(0.0) == r.build_time
                })
                .cloned()
                .unwrap_or_else(|| UnitSummary::from_target_stats(r))
        };

        let items = queue
            .tasks
            .into_iter()
            .map(|task| ConstructionItem {
                id: task.id,
                builders: task.builders.iter().map(find_unit).collect(),
                targets: task.targets.iter().map(find_unit).collect(),
                start_after: task.start_after,
            })
            .collect();

        Self {
            eco: queue.initial_eco,
            items,
        }
    }

    /// Convert a simulation queue into a plan using only the runtime stats.
    ///
    /// Display fields will be empty; this is useful for headless tooling that
    /// only needs the numeric plan shape.
    pub fn from_build_queue(queue: BuildQueue) -> Self {
        let items = queue
            .tasks
            .into_iter()
            .map(|task| ConstructionItem {
                id: task.id,
                builders: task
                    .builders
                    .iter()
                    .map(UnitSummary::from_builder_stats)
                    .collect(),
                targets: task
                    .targets
                    .iter()
                    .map(UnitSummary::from_target_stats)
                    .collect(),
                start_after: task.start_after,
            })
            .collect();

        Self {
            eco: queue.initial_eco,
            items,
        }
    }
}
