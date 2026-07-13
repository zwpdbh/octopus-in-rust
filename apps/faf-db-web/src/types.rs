use faf_sim::sim::{BuildQueue, BuildTask, UnitDefRef};
use faf_sim::{Energy, EnergyRate, Mass, MassRate, Storage, Time};
use serde::{Deserialize, Serialize};

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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EcoInitialSettings {
    pub mass_income: MassRate,
    pub energy_income: EnergyRate,
    pub mass_storage: Storage<Mass>,
    pub energy_storage: Storage<Energy>,
}

impl Default for EcoInitialSettings {
    fn default() -> Self {
        Self {
            mass_income: MassRate::from_raw(1.0),
            energy_income: EnergyRate::from_raw(20.0),
            mass_storage: Storage::new(Mass::from_raw(650.0), Mass::from_raw(650.0)),
            energy_storage: Storage::new(Energy::from_raw(4000.0), Energy::from_raw(4000.0)),
        }
    }
}

impl EcoInitialSettings {
    pub fn to_economy_state(&self) -> faf_sim::EconomyState {
        faf_sim::EconomyState {
            mass_income: self.mass_income,
            energy_income: self.energy_income,
            mass_storage: self.mass_storage,
            energy_storage: self.energy_storage,
        }
    }

    pub fn from_economy_state(state: &faf_sim::EconomyState) -> Self {
        Self {
            mass_income: state.mass_income,
            energy_income: state.energy_income,
            mass_storage: state.mass_storage,
            energy_storage: state.energy_storage,
        }
    }
}

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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ConstructionPlan {
    #[serde(rename = "initial_eco", alias = "eco")]
    pub eco: EcoInitialSettings,
    pub items: Vec<ConstructionItem>,
}

impl ConstructionPlan {
    /// Convert the UI plan into the simulation queue format.
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
                    .map(|u| UnitDefRef {
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
                    .map(|t| UnitDefRef {
                        build_power: 0.0,
                        mass_cost: t.build_cost_mass.unwrap_or(0.0),
                        energy_cost: t.build_cost_energy.unwrap_or(0.0),
                        build_time: t.build_time.unwrap_or(0.0),
                        unit_id: Some(t.id.clone()),
                        ..Default::default()
                    })
                    .collect(),
            })
            .collect();

        BuildQueue {
            initial_eco: self.eco.to_economy_state(),
            tasks,
        }
    }

    pub fn from_build_queue(queue: BuildQueue, units: &[UnitSummary]) -> Self {
        let unit_map: std::collections::HashMap<&str, &UnitSummary> =
            units.iter().map(|u| (u.id.as_str(), u)).collect();

        let find_unit = |r: &UnitDefRef| -> UnitSummary {
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
                .unwrap_or_else(|| UnitSummary {
                    id: r.unit_id.clone().unwrap_or_default(),
                    display_name: String::new(),
                    faction: String::new(),
                    tech: String::new(),
                    category: String::new(),
                    strategic_icon_name: None,
                    kind: String::new(),
                    build_rate: Some(r.build_power),
                    build_cost_mass: Some(r.mass_cost),
                    build_cost_energy: Some(r.energy_cost),
                    build_time: Some(r.build_time),
                })
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
            eco: EcoInitialSettings::from_economy_state(&queue.initial_eco),
            items,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct UnitDetailData {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub name_zh: Option<String>,
    #[serde(default)]
    pub description_zh: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub general: Option<GeneralDetail>,
    #[serde(default)]
    pub economy: Option<EconomyDetail>,
    #[serde(default)]
    pub defense: Option<DefenseDetail>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct GeneralDetail {
    #[serde(default)]
    pub unit_name: Option<String>,
    #[serde(default)]
    pub faction_name: Option<String>,
    #[serde(default)]
    pub tech_level: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct EconomyDetail {
    #[serde(default)]
    pub build_cost_energy: Option<f64>,
    #[serde(default)]
    pub build_cost_mass: Option<f64>,
    #[serde(default)]
    pub build_time: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct DefenseDetail {
    #[serde(default)]
    pub max_health: Option<f64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AssignmentTarget {
    ExistingBuilder { item_id: u32 },
    ExistingTarget { item_id: u32 },
    NewBuilder,
    NewTarget,
}

impl AssignmentTarget {
    pub fn accepts(self, unit: &UnitSummary) -> bool {
        match self {
            AssignmentTarget::ExistingBuilder { .. } | AssignmentTarget::NewBuilder => {
                unit.category == "Construction - Buildpower"
            }
            _ => true,
        }
    }
}

/// Runtime state of the simulation UI.
///
/// This is intentionally richer than the wire protocol's `SimRuntimeStatus`
/// because the frontend needs to distinguish "the user never started" (Idle),
/// "the build queue finished naturally" (Finished), and "the user stopped it"
/// (Idle) for control and visibility purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimulationUiState {
    NotStartYet,
    Running,
    Paused,
    Finished,
}
