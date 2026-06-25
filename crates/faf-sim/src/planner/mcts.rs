//! Monte Carlo Tree Search planner guided by a learned value network.
//!
//! This module is a placeholder for the MCTS + value-net approach described in
//! `crates/faf-sim/doc/06-mcts-value-net-plan.md`. The public API mirrors the
//! other strategy modules (`greedy`, `beam`) so it can be slotted into
//! [`crate::planner::Planner`] via [`Strategy::Mcts`](crate::planner::Strategy::Mcts).

use faf_units::{DataIndex, Unit};

use crate::planner::core::{PlanResult, PlannerConfig, PlannerError};
use crate::sim::GraphState;

/// Run MCTS from `initial_state` toward `goal`.
///
/// Currently a placeholder. It will eventually expand a UCT search tree,
/// evaluate leaves with a learned value network, and return the best plan.
pub fn plan(
    _index: &DataIndex,
    _initial_state: GraphState,
    _goal: &Unit,
    _iterations: usize,
    _config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    todo!("MCTS + value-net planner is not yet implemented")
}
