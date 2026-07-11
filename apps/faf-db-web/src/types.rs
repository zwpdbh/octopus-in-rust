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
    pub eco: EcoInitialSettings,
    pub items: Vec<ConstructionItem>,
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
