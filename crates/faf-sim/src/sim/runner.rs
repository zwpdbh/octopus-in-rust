//! High-level async driver for reactive build-order simulation.
//!
//! This module hides the actor wiring, channel setup, and time-advance loop
//! behind a single async function. The caller provides a `Planner`, a `Units`
//! repository, and a goal unit; the runner returns the final simulation state.

use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time;

use crate::decision_actor::DecisionActor;
use crate::message::{Command, Observation};
use crate::planner::Planner;
use crate::sim::state::{GraphSimError, GraphState};
use crate::sim_actor::SimActor;
use crate::units::{UnitKind, Units};

/// Default simulation timestep in seconds.
const DEFAULT_SIM_DT: f64 = 10.0;
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
    /// Create a default configuration for a given strategy.
    ///
    /// Uses `Planner::reactive` and a 10-second simulation timestep.
    pub fn for_strategy(strategy: crate::planner::Strategy) -> Self {
        Self {
            planner: Planner::reactive(strategy),
            sim_dt: DEFAULT_SIM_DT,
            max_sim_time: DEFAULT_MAX_SIM_TIME,
        }
    }
}

/// Result of a completed reactive simulation.
#[derive(Debug)]
pub struct SimulationResult {
    /// Final authoritative simulation state.
    pub final_state: GraphState,
    /// Number of simulation ticks that elapsed.
    pub elapsed_ticks: usize,
}

/// Errors that can occur while running a simulation.
#[derive(Debug)]
pub enum SimulationError {
    /// The simulation did not reach the goal within the time limit.
    Timeout { max_sim_time: f64 },
    /// The simulation actor returned an error.
    Sim(GraphSimError),
    /// The simulation task panicked or was cancelled.
    TaskJoin(tokio::task::JoinError),
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
            SimulationError::TaskJoin(e) => Some(e),
            _ => None,
        }
    }
}

impl From<GraphSimError> for SimulationError {
    fn from(value: GraphSimError) -> Self {
        SimulationError::Sim(value)
    }
}

impl From<tokio::task::JoinError> for SimulationError {
    fn from(value: tokio::task::JoinError) -> Self {
        SimulationError::TaskJoin(value)
    }
}

/// Run a reactive build-order simulation.
///
/// This function pauses Tokio's virtual clock, spawns a `SimActor` and a
/// `DecisionActor`, and drives time forward in `config.sim_dt` increments until
/// the goal is reached or `config.max_sim_time` is exceeded.
///
/// # Panics
///
/// Panics if `config.sim_dt` is not positive.
pub async fn run_build_order_simulation(
    units: Units,
    goal: UnitKind,
    config: SimulationConfig,
) -> Result<SimulationResult, SimulationError> {
    assert!(config.sim_dt > 0.0, "sim_dt must be positive");

    // Pause Tokio's clock so the actor loop can be driven forward
    // deterministically without waiting for real wall-clock time.
    time::pause();

    let max_ticks = (config.max_sim_time / config.sim_dt) as usize + 1;
    let tick = Duration::from_secs_f64(config.sim_dt);

    let (obs_tx, obs_rx) = mpsc::channel::<Observation>(64);
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);

    let sim = SimActor::new(
        &[UnitKind::Commander],
        units.clone(),
        Some(goal.clone()),
        config.sim_dt,
        obs_tx,
        cmd_rx,
    );
    let sim_handle = tokio::spawn(sim.run());

    let decision_actor = DecisionActor::new(config.planner, units, goal, obs_rx, cmd_tx);
    let planner_handle = tokio::spawn(decision_actor.run());

    // Drive the simulation timer forward until the goal is reached or we hit
    // the safety cap.
    let mut ticks = 0;
    while !sim_handle.is_finished() {
        if ticks >= max_ticks {
            sim_handle.abort();
            planner_handle.abort();
            return Err(SimulationError::Timeout {
                max_sim_time: config.max_sim_time,
            });
        }
        time::advance(tick).await;
        ticks += 1;
    }

    let final_state = match sim_handle.await {
        Ok(Ok(state)) => state,
        Ok(Err(e)) => return Err(e.into()),
        Err(e) => return Err(e.into()),
    };

    // The planner actor exits once the observation channel is closed.
    let _ = planner_handle.await;

    if final_state.time > config.max_sim_time {
        return Err(SimulationError::Timeout {
            max_sim_time: config.max_sim_time,
        });
    }

    Ok(SimulationResult {
        final_state,
        elapsed_ticks: ticks,
    })
}

#[cfg(test)]
mod tests {
    // MCTS simulation tests will be added once the MCTS planner is implemented.
}
