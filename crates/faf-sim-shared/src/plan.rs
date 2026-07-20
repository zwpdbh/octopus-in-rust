//! Shared plan types that bridge the simulator and UI/tooling.

use faf_sim::runtime::{BuildQueue, BuildTask, EcoSnapshot, UnitEcoStats};
use faf_sim::{EconomyRuntimeState, Energy, EnergyRate, Mass, MassRate, Storage, Time};
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

/// Initial economy settings, serializable wrapper around the runtime state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EcoInitialSettings {
    pub production_per_second_mass: MassRate,
    pub production_per_second_energy: EnergyRate,
    #[serde(default)]
    pub maintenance_consumption_per_second_energy: EnergyRate,
    pub mass_storage: Storage<Mass>,
    pub energy_storage: Storage<Energy>,
}

impl Default for EcoInitialSettings {
    fn default() -> Self {
        Self {
            production_per_second_mass: MassRate::from_raw(1.0),
            production_per_second_energy: EnergyRate::from_raw(20.0),
            maintenance_consumption_per_second_energy: EnergyRate::from_raw(0.0),
            mass_storage: Storage::new(Mass::from_raw(650.0), Mass::from_raw(650.0)),
            energy_storage: Storage::new(Energy::from_raw(4000.0), Energy::from_raw(4000.0)),
        }
    }
}

impl EcoInitialSettings {
    pub fn to_runtime_state(&self) -> EconomyRuntimeState {
        EconomyRuntimeState {
            production_per_second_mass: self.production_per_second_mass,
            production_per_second_energy: self.production_per_second_energy,
            maintenance_consumption_per_second_energy: self
                .maintenance_consumption_per_second_energy,
            mass_storage: self.mass_storage,
            energy_storage: self.energy_storage,
        }
    }

    pub fn to_snapshot(&self) -> EcoSnapshot {
        EcoSnapshot {
            time: 0.0,
            production_per_second_mass: self.production_per_second_mass.value(),
            production_per_second_energy: self.production_per_second_energy.value(),
            maintenance_consumption_per_second_energy: self
                .maintenance_consumption_per_second_energy
                .value(),
            mass_drain: 0.0,
            energy_drain: 0.0,
            total_mass_spent: 0.0,
            total_energy_spent: 0.0,
            mass_storage: self.mass_storage.current.value(),
            mass_storage_cap: self.mass_storage.cap.value(),
            energy_storage: self.energy_storage.current.value(),
            energy_storage_cap: self.energy_storage.cap.value(),
        }
    }

    pub fn from_runtime_state(state: &EconomyRuntimeState) -> Self {
        Self {
            production_per_second_mass: state.production_per_second_mass,
            production_per_second_energy: state.production_per_second_energy,
            maintenance_consumption_per_second_energy: state
                .maintenance_consumption_per_second_energy,
            mass_storage: state.mass_storage,
            energy_storage: state.energy_storage,
        }
    }
}

/// One item in a construction plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConstructionItem {
    pub id: u32,
    pub builders: Vec<UnitSummary>,
    pub targets: Vec<UnitSummary>,
    pub start_after: Time,
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
    pub eco: EcoInitialSettings,
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
            initial_eco: self.eco.to_runtime_state(),
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
            eco: EcoInitialSettings::from_runtime_state(&queue.initial_eco),
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
            eco: EcoInitialSettings::from_runtime_state(&queue.initial_eco),
            items,
        }
    }
}
