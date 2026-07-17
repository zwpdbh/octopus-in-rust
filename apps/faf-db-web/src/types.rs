use serde::Deserialize;
use std::collections::HashMap;

pub use faf_dioxus_ui::components::GraphData;
pub use faf_sim::units::BlueprintGraph;
pub use faf_sim_shared::plan::{
    ConstructionItem, ConstructionPlan, EcoInitialSettings, UnitSummary,
};

/// Server response for the blueprint graph endpoint: raw symbolic graph plus a
/// unit summary for every shown node.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct BlueprintGraphResponse {
    pub graph: BlueprintGraph,
    /// Concrete unit details for every shown graph node, keyed by node id.
    ///
    /// The graph itself is symbolic: nodes only carry abstract [`UnitKind`]
    /// identities and rendering metadata. The UI needs real blueprint data
    /// (id, faction, tech, build costs, economy, portrait) for the detail
    /// panel on node click and for stable node identity. The server resolves
    /// those from the raw unit index and sends them here so the frontend does
    /// not need its own copy of the unit database.
    pub summaries: HashMap<String, UnitSummary>,
}

/// Rendering payload for the blueprint graph component.
pub type BlueprintGraphData = GraphData<UnitSummary, ()>;

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
