//! Input types for the build scheduler.

use faf_sim::runtime::EcoSnapshot;
use faf_sim::units::UnitKind;
use serde::{Deserialize, Serialize};

/// Lower-bound thresholds that define an eco goal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EcoTarget {
    pub mass_production: Option<f64>,
    pub energy_production: Option<f64>,
    pub mass_storage_cap: Option<f64>,
    pub energy_storage_cap: Option<f64>,
    pub tolerance: f64,
}

impl EcoTarget {
    /// True if every set threshold is met by the given snapshot.
    pub fn is_reached(&self, eco: &EcoSnapshot) -> bool {
        let tol = self.tolerance;
        self.mass_production
            .is_none_or(|t| eco.production_per_second_mass + tol >= t)
            && self
                .energy_production
                .is_none_or(|t| eco.production_per_second_energy + tol >= t)
            && self
                .mass_storage_cap
                .is_none_or(|t| eco.mass_storage_cap + tol >= t)
            && self
                .energy_storage_cap
                .is_none_or(|t| eco.energy_storage_cap + tol >= t)
    }
}

/// Search budget and simulator caps.
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

/// Request to reach an eco target as quickly as possible.
#[derive(Debug, Clone, PartialEq)]
pub struct EcoScheduleRequest {
    pub initial_eco: EcoSnapshot,
    pub initial_inventory: Vec<UnitKind>,
    pub target: EcoTarget,
    pub options: SearchOptions,
}

/// Request to build a target unit as quickly as possible.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitScheduleRequest {
    pub initial_eco: EcoSnapshot,
    pub initial_inventory: Vec<UnitKind>,
    pub target: UnitKind,
    pub options: SearchOptions,
}

/// CLI-friendly input file format for `schedule eco`.
///
/// The target is intentionally simple: just target income thresholds for mass
/// and/or energy production.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EcoScheduleInput {
    pub initial_eco: EcoSnapshot,
    #[serde(default = "default_inventory")]
    pub initial_inventory: Vec<String>,
    #[serde(default)]
    pub target_mass_production: Option<f64>,
    #[serde(default)]
    pub target_energy_production: Option<f64>,
    #[serde(default = "default_tolerance")]
    pub tolerance: f64,
    #[serde(default)]
    pub options: SearchOptions,
}

fn default_tolerance() -> f64 {
    1.0
}

/// CLI-friendly input file format for `schedule unit`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitScheduleInput {
    pub initial_eco: EcoSnapshot,
    #[serde(default = "default_inventory")]
    pub initial_inventory: Vec<String>,
    pub target: String,
    #[serde(default)]
    pub options: SearchOptions,
}

fn default_inventory() -> Vec<String> {
    vec!["Commander".to_string()]
}
