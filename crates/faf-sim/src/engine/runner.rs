//! Stub for the previous actor-based reactive build-order simulation driver.
//!
//! The actor-based reactive loop has been removed. The deterministic core is now
//! split into [`EcoEngine`](crate::engine::EcoEngine) (unit-agnostic economy and
//! clock) and [`UnitGraph`](crate::engine::UnitGraph) (unit/build-order state).
//! A future higher-level simulation layer will coordinate them to drive reactive
//! episodes.
//!
//! This module keeps the public config/result types for backwards compatibility
//! while the migration is in progress; `run_build_order_simulation` is stubbed
//! with `todo!()`.

use crate::engine::simulation_state::SimulationState;
use crate::planner::core::Goal;
use crate::planner::Planner;
use crate::units::Units;

/// Default simulation timestep in seconds.
const DEFAULT_SIM_DT: f64 = 1.0;
/// Default maximum in-game time in seconds (8 hours).
const DEFAULT_MAX_SIM_TIME: f64 = 8.0 * 60.0 * 60.0;

/// Configuration for a reactive build-order simulation.
#[derive(Debug, Clone)]
pub struct SimulationConfig {
    /// Planner used to make decisions each tick.
    pub planner: Planner,
    /// Simulation timestep in seconds.
    pub sim_dt: f64,
    /// Maximum in-game time before the simulation is aborted.
    pub max_sim_time: f64,
}

impl SimulationConfig {
    /// Create a default configuration for a given strategy and value net.
    ///
    /// Uses `Planner::reactive` and a 1-second simulation timestep.
    pub fn for_strategy(
        strategy: crate::planner::Strategy,
        value_net: Box<dyn crate::planner::policy::value_net::ValueNet>,
    ) -> Self {
        Self {
            planner: Planner::reactive(strategy, value_net),
            sim_dt: DEFAULT_SIM_DT,
            max_sim_time: DEFAULT_MAX_SIM_TIME,
        }
    }
}

/// Result of a completed reactive simulation.
#[derive(Debug)]
pub struct SimulationResult {
    /// Final authoritative simulation state.
    pub final_state: SimulationState,
    /// Number of simulation ticks that elapsed.
    pub elapsed_ticks: usize,
}

/// Errors that can occur while running a simulation.
#[derive(Debug)]
pub enum SimulationError {
    /// The simulation did not reach the goal within the time limit.
    Timeout { max_sim_time: f64 },
    /// The simulation state rejected an action.
    Sim(crate::engine::unit_graph::GraphSimError),
    /// The simulation task panicked or was cancelled.
    TaskJoin(String),
}

impl std::fmt::Display for SimulationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SimulationError::Timeout { max_sim_time } => write!(
                f,
                "simulation did not reach the goal within {:.1} hours of in-game time",
                max_sim_time / 3600.0
            ),
            SimulationError::Sim(e) => write!(f, "simulation error: {}", e),
            SimulationError::TaskJoin(e) => write!(f, "simulation task failed: {}", e),
        }
    }
}

impl std::error::Error for SimulationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SimulationError::Sim(e) => Some(e),
            _ => None,
        }
    }
}

impl From<crate::engine::unit_graph::GraphSimError> for SimulationError {
    fn from(value: crate::engine::unit_graph::GraphSimError) -> Self {
        SimulationError::Sim(value)
    }
}

/// Run a reactive build-order simulation.
///
/// This function is temporarily stubbed. The previous actor-based
/// implementation has been removed. The replacement will be a simulation layer
/// that coordinates [`EcoEngine`](crate::engine::EcoEngine) and
/// [`UnitGraph`](crate::engine::UnitGraph).
pub async fn run_build_order_simulation(
    _units: Units,
    _goal: Goal,
    _config: SimulationConfig,
) -> Result<SimulationResult, SimulationError> {
    todo!("run_build_order_simulation is stubbed; migrate to EcoEngine")
}
