//! Reward shaping for policy-gradient training.

use crate::planner::plan_graph::PlanGraph;
use crate::sim::GraphState;
use crate::units::{TechLevel, UnitKind, Units};

/// Compute a shaped reward from the final state of an episode.
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
    reward += factory_tier * 3.0;
    reward += engineer_tier * 3.0;

    // Economy scale: more build power and income let the agent execute faster.
    reward += (state.total_active_build_power(units) as f32 / 50.0).clamp(0.0, 5.0);
    reward += (state.economy.net_mass_income as f32 / 50.0).clamp(0.0, 5.0);
    reward += (state.economy.net_energy_income as f32 / 200.0).clamp(0.0, 5.0);

    // Large bonus for reaching the goal, with a time premium for faster runs.
    if state.goal_reached(goal) {
        reward += 100.0;
        reward += 1000.0 / (1.0 + state.time as f32);
    } else {
        // Penalty for failing to reach the goal within the step budget,
        // so the agent prefers finishing episodes rather than stalling.
        reward -= 5.0;
    }

    reward
}
