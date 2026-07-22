use faf_quantities::MassRate;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub use faf_blueprints::UnitKind;
pub use faf_dioxus_ui::components::GraphData;
pub use faf_sim_shared::plan_types::{
    ConstructionItem, ConstructionPlan, EcoInitialSettings, UnitSummary,
};
pub use faf_sim_shared::{
    Action, DirectionScores, EcoSnapshot, PriorityTable, Schedule, ScheduleWithReasoning,
    StepReasoning, StepResult,
};

// ---------------------------------------------------------------------------
// Scheduling wire types (from faf-sim-shared's /api/schedule protocol).
//
// These types live in `faf-sim-shared` so the backend and frontend share a
// single serialized format and cannot drift out of sync.
// ---------------------------------------------------------------------------

/// Search budget and simulator caps for a scheduling request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchOptions {
    pub max_search_seconds: f64,
    pub simulation_max_time_seconds: f64,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            max_search_seconds: 2.0,
            simulation_max_time_seconds: 6000.0,
        }
    }
}

#[allow(dead_code)]
fn default_max_mex_count() -> u32 {
    10
}

/// Scheduling request sent to `POST /api/schedule`. Tagged by `mode`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum ScheduleApiRequest {
    Eco {
        initial_eco: EcoSnapshot,
        initial_inventory: Vec<UnitKind>,
        target_mass_production: MassRate,
        tolerance: f64,
        options: SearchOptions,
        #[serde(default = "default_max_mex_count")]
        max_mex_count: u32,
    },
    Unit {
        initial_eco: EcoSnapshot,
        initial_inventory: Vec<UnitKind>,
        target: UnitKind,
        options: SearchOptions,
        #[serde(default = "default_max_mex_count")]
        max_mex_count: u32,
    },
}

/// Error envelope returned when scheduling fails.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ScheduleApiError {
    pub error: String,
}

/// Runtime state of the scheduling panel on the scheduler page.
#[derive(Debug, Clone, PartialEq)]
pub enum ScheduleUiState {
    Idle,
    Computing,
    Success(Schedule, Vec<StepReasoning>),
    Failed(String),
}

// ---------------------------------------------------------------------------
// Concrete blueprint relationship graph (mirror of faf-db-server's
// /api/blueprint-graph protocol).
// ---------------------------------------------------------------------------

/// Economic/builder role of a concrete node; drives node color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EconRole {
    Commander,
    Engineer,
    Factory,
    Mex,
    Pgen,
    MassStorage,
    EnergyStorage,
    Experimental,
}

impl EconRole {
    pub fn color(self) -> &'static str {
        match self {
            EconRole::Commander => "#fbbf24",
            EconRole::Engineer => "#60a5fa",
            EconRole::Factory => "#a78bfa",
            EconRole::Mex => "#34d399",
            EconRole::Pgen => "#f87171",
            EconRole::MassStorage => "#2dd4bf",
            EconRole::EnergyStorage => "#f472b6",
            EconRole::Experimental => "#f97316",
        }
    }
}

/// A concrete unit node in the relationship graph.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ConcreteGraphNode {
    /// Blueprint id, e.g. "UEL0105".
    pub id: String,
    pub display_name: String,
    pub faction: String,
    pub tech: String,
    pub role: EconRole,
    /// Dagre layer: ACU=0, T1=1, T2=2, T3=3, Experimental=4.
    pub layer: i32,
    /// Abstract kind this concrete unit maps to (needed by schedule requests).
    pub kind: UnitKind,
}

/// The kind of a directed edge between two concrete units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConcreteEdgeKind {
    BuiltBy,
    UpgradesInto,
}

/// A directed edge from `source` (builder / lower tier) to `target`.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ConcreteGraphEdge {
    pub source: String,
    pub target: String,
    pub kind: ConcreteEdgeKind,
}

/// Server response for the blueprint graph endpoint: the concrete unit
/// relationship graph plus a unit summary for every node.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct BlueprintGraphResponse {
    pub nodes: Vec<ConcreteGraphNode>,
    pub edges: Vec<ConcreteGraphEdge>,
    /// Unit summaries keyed by blueprint id.
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
