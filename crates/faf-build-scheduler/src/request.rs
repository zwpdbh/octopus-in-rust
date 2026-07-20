//! Input types for the build scheduler.

use bevy_ecs::prelude::Resource;
use faf_blueprints::UnitKind;
use faf_quantities::MassRate;
use faf_sim_shared::EcoSnapshot;
use serde::{Deserialize, Serialize};

use crate::config::SchedulerConfig;

/// Lower-bound threshold that defines an eco goal.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EcoTarget {
    /// Target mass income per second.
    pub mass_production: MassRate,
    /// Tolerance applied when checking whether the target is reached.
    pub tolerance: f64,
}

impl EcoTarget {
    /// True if the snapshot's mass income meets the target within tolerance.
    pub fn is_reached(&self, eco: &EcoSnapshot) -> bool {
        eco.production_per_second_mass + self.tolerance >= self.mass_production.value()
    }
}

/// Search budget and simulator caps.
#[derive(Debug, Clone, PartialEq, Resource, Serialize, Deserialize)]
pub struct SearchOptions {
    pub max_search_seconds: f64,
    pub simulation_max_time_seconds: f64,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,
    #[serde(default = "default_max_steps")]
    pub max_steps: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            max_search_seconds: 2.0,
            simulation_max_time_seconds: 6000.0,
            max_iterations: default_max_iterations(),
            max_steps: default_max_steps(),
        }
    }
}

fn default_max_iterations() -> usize {
    1_000
}

fn default_max_steps() -> usize {
    1000
}

/// Request to reach an eco target as quickly as possible.
#[derive(Debug, Clone, PartialEq)]
pub struct EcoScheduleRequest {
    pub initial_eco: EcoSnapshot,
    pub initial_inventory: Vec<UnitKind>,
    pub target: EcoTarget,
    pub options: SearchOptions,
    pub config: SchedulerConfig,
}

/// Request to build a target unit as quickly as possible.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitScheduleRequest {
    pub initial_eco: EcoSnapshot,
    pub initial_inventory: Vec<UnitKind>,
    pub target: UnitKind,
    pub options: SearchOptions,
    pub config: SchedulerConfig,
}

/// CLI-friendly input file format for `schedule eco`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EcoScheduleInput {
    pub initial_eco: EcoSnapshot,
    pub initial_inventory: Vec<String>,
    pub target_mass_production: MassRate,
    pub tolerance: f64,
    #[serde(default)]
    pub options: SearchOptions,
    #[serde(default)]
    pub config: SchedulerConfig,
}

/// CLI-friendly input file format for `schedule unit`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitScheduleInput {
    pub initial_eco: EcoSnapshot,
    pub initial_inventory: Vec<String>,
    pub target: String,
    #[serde(default)]
    pub options: SearchOptions,
    #[serde(default)]
    pub config: SchedulerConfig,
}
