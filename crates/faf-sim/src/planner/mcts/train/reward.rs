//! Reward shaping for policy-gradient training.

use crate::planner::plan_graph::PlanGraph;
use crate::sim::GraphState;
use crate::units::{TechLevel, UnitKind, Units};

/// Compute a shaped reward from the final state of an episode.
///
/// The reward is designed to encourage fast goal completion while still giving
/// a strong signal for the economic infrastructure (mass, power, build power,
/// energy storage) that makes fast completions possible.
pub(crate) fn compute_progress_reward(
    state: &GraphState,
    units: &Units,
    goal: &UnitKind,
    plan: &PlanGraph,
) -> f32 {
    let mut reward = 0.0f32;

    // Reward for owning nodes on the plan graph. Nodes closer to the goal
    // contribute more, so the policy is pushed toward the goal path.
    for node in plan.graph().node_indices() {
        let kind = &plan.graph()[node];
        if state.has_completed_unit(kind) {
            let distance = crate::planner::mcts::features::distance_to_goal(plan, kind);
            reward += 2.0 / (1.0 + distance as f32);
        }
    }

    // Tech-tier milestones: unlocking higher-tier factories and engineers is
    // critical for reaching the goal.
    let factory_tier = [TechLevel::T1, TechLevel::T2, TechLevel::T3]
        .iter()
        .filter(|t| state.has_completed_unit(&UnitKind::Factory(**t)))
        .count() as f32;
    let engineer_tier = [TechLevel::T1, TechLevel::T2, TechLevel::T3]
        .iter()
        .filter(|t| state.has_completed_unit(&UnitKind::Engineer(**t)))
        .count() as f32;
    reward += factory_tier * 5.0;
    reward += engineer_tier * 5.0;

    // Economic infrastructure: the agent should learn to build mass, power,
    // energy storage, and engineers. These rewards scale with counts and income
    // so that building *more* eco is directly rewarded, not just unlocking the
    // first instance of each unit.
    let mex_count = state.count_active_mex() as f32;
    let pgen_count = state.count_active_pgen() as f32;
    let storage_count = state.count_active_energy_storage() as f32;
    let build_power = state.total_active_build_power(units) as f32;

    reward += mex_count * 2.0;
    reward += pgen_count * 0.5;
    reward += storage_count * 0.25;
    reward += (build_power / 20.0).clamp(0.0, 50.0);

    // Income: direct incentives for high mass/energy throughput. These are
    // unbounded so the policy does not hit an artificial cap too early.
    reward += (state.economy.net_mass_income as f32 / 5.0).clamp(0.0, 100.0);
    reward += (state.economy.net_energy_income as f32 / 50.0).clamp(0.0, 100.0);

    // Goal completion: the dominant term. Faster completions get much higher
    // reward, which should drive the policy to invest in the eco needed to
    // achieve them.
    if state.goal_reached(goal) {
        reward += 2500.0;
        // Subtract roughly 0.4 reward per second of game time. At 33m this is
        // still a large positive bonus; only very slow runs approach zero.
        reward -= state.time as f32 * 0.4;
    } else {
        // Strong penalty for failing to reach the goal within the step budget,
        // so the agent prefers finishing episodes rather than stalling.
        reward -= 50.0;
    }

    reward
}
