//! Simulator-based rollout helpers for training rewards.
//!
//! These functions run short, self-contained simulations from a given state to
//! estimate how good an action was. They never mutate the caller's state.

use crate::engine::simulation_state::GoalProject;
use crate::engine::simulation_state::SimulationState;
use crate::engine::unit_graph::builder_power;
use crate::planner::core::Goal;
use crate::units::Units;

use super::config::TrainConfig;

/// Result of a single rollout.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct RolloutResult {
    /// Total mass actually spent during the rollout.
    pub mass_spent: f32,
    /// Longest contiguous energy-stall duration in seconds.
    pub longest_energy_stall_secs: f32,
    /// Whether the final goal completed during the rollout.
    pub goal_finished: bool,
    /// Time in seconds until goal completion, if it finished.
    pub time_to_finish_secs: Option<f32>,
    /// Whether stored mass exceeded the hoarding ratio at the end of the rollout.
    pub mass_hoarded: bool,
}

/// Run a phantom-goal rollout to measure how much mass the economy can spend.
///
/// The rollout creates a fake goal project that consumes resources like the real
/// goal but can never complete. 80% of total build power is assigned to it.
/// The caller keeps its original state because this function clones it.
pub(crate) fn eco_rollout(
    initial_state: &SimulationState,
    units: &Units,
    config: &TrainConfig,
    goal: &Goal,
) -> RolloutResult {
    let mut state = initial_state.clone();
    let horizon = config.eco_rollout_horizon_secs;
    let dt = config.dt;

    // Assign a fraction of total BP to the phantom goal.
    let builders = select_builders_by_power(&state, units, config.rollout_bp_fraction);
    if !builders.is_empty() {
        state.goal_project = Some(GoalProject {
            goal: *goal,
            remaining_work: f64::MAX,
            started_by: builders,
            assisted_by: Vec::new(),
            completed: false,
        });
    }

    run_rollout(&mut state, units, dt, horizon, false, config)
}

/// Run a real-goal rollout to see if the goal finishes within the cap.
///
/// The state is expected to already contain an active real goal project. Extra
/// builders are assigned to it up to the configured BP fraction.
pub(crate) fn rush_rollout(
    initial_state: &SimulationState,
    units: &Units,
    config: &TrainConfig,
) -> RolloutResult {
    let mut state = initial_state.clone();
    let cap = config.rush_rollout_cap_secs;
    let dt = config.dt;

    // Add more builders to the real goal project if available.
    // Select builders first to avoid borrowing `state` mutably and immutably at
    // the same time.
    let extra_builders = select_builders_by_power(&state, units, config.rollout_bp_fraction);
    if let Some(ref mut gp) = state.goal_project {
        for b in extra_builders {
            if !gp.started_by.contains(&b) && !gp.assisted_by.contains(&b) {
                gp.assisted_by.push(b);
            }
        }
    }

    run_rollout(&mut state, units, dt, cap, true, config)
}

/// Simulate `state` forward for up to `horizon` seconds.
///
/// If `stop_on_goal_completion` is true, the simulation stops as soon as the
/// goal project is marked completed.
fn run_rollout(
    state: &mut SimulationState,
    units: &Units,
    dt: f64,
    horizon: f32,
    stop_on_goal_completion: bool,
    config: &TrainConfig,
) -> RolloutResult {
    let mut result = RolloutResult::default();
    let mut current_stall_secs = 0.0f32;
    let start_time = state.time;

    let steps = (horizon / dt as f32).ceil() as usize;
    for _ in 0..steps {
        if stop_on_goal_completion && state.goal_reached_from_project() {
            result.goal_finished = true;
            result.time_to_finish_secs = Some((state.time - start_time) as f32);
            break;
        }

        let prev_mass_storage = state.economy.mass_storage.value();
        let mass_income = state.economy.net_mass_income.value() * dt;

        state.tick(units, dt);

        let mass_spent_this_tick =
            (prev_mass_storage + mass_income - state.economy.mass_storage.value()).max(0.0);
        result.mass_spent += mass_spent_this_tick as f32;

        if is_energy_stalled(state) {
            current_stall_secs += dt as f32;
            result.longest_energy_stall_secs =
                result.longest_energy_stall_secs.max(current_stall_secs);
        } else {
            current_stall_secs = 0.0;
        }
    }

    result.mass_hoarded = state.economy.mass_storage.value()
        > state.economy.mass_storage_cap.value() * (config.mass_storage_hoarding_ratio as f64);

    result
}

/// True when energy storage is empty and the economy is draining more than it
/// produces.
fn is_energy_stalled(state: &SimulationState) -> bool {
    state.economy.energy_storage.value() <= 1e-6
}

/// Select active builders whose combined build power reaches `fraction` of the
/// state's total active build power.
///
/// Builders are chosen highest-tech first. They may currently be busy; this is
/// intentional because the rollout is a capacity test, not a literal schedule.
fn select_builders_by_power(
    state: &SimulationState,
    units: &Units,
    fraction: f32,
) -> Vec<crate::engine::unit_graph::NodeId> {
    let total_bp = state.total_active_build_power(units);
    if total_bp <= 0.0 {
        return Vec::new();
    }
    let target_bp = total_bp * fraction as f64;

    let mut builders: Vec<_> = state
        .active_units()
        .into_iter()
        .filter(|&id| builder_power(id, &state.graph, units) > 0.0)
        .collect();

    builders.sort_by(|&a, &b| {
        let rate_a = builder_power(a, &state.graph, units);
        let rate_b = builder_power(b, &state.graph, units);
        rate_b
            .partial_cmp(&rate_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut selected = Vec::new();
    let mut accumulated = 0.0;
    for id in builders {
        if accumulated >= target_bp {
            break;
        }
        accumulated += builder_power(id, &state.graph, units);
        selected.push(id);
    }
    selected
}

/// Extension trait for goal-project checks used during rollouts.
trait RolloutGoalExt {
    /// True if the active goal project is completed.
    fn goal_reached_from_project(&self) -> bool;
}

impl RolloutGoalExt for SimulationState {
    fn goal_reached_from_project(&self) -> bool {
        self.goal_project.as_ref().is_some_and(|p| p.completed)
    }
}
