//! Monte Carlo Tree Search planner guided by a learned value network.
//!
//! This module is a placeholder scaffold for the MCTS + value-net approach
//! described in `crates/faf-sim/doc/06-mcts-value-net-plan.md`. The public API
//! mirrors the other strategy modules (`greedy`, `beam`) so it can be slotted
//! into [`crate::planner::Planner`] via [`Strategy::Mcts`](crate::planner::Strategy::Mcts).
//!
//! # Basic flow
//!
//! 1. `features::featurize` converts a `GraphState` into a fixed-size `Vec<f32>`.
//! 2. `value_net::ValueNet` turns those features into a scalar value estimate.
//! 3. `search::MctsSearch` runs UCT search, using the value net to evaluate
//!    leaf nodes.
//! 4. `plan` dispatches the search and returns the best `PlanResult`.
//!
//! All heavy details are currently `todo!()` so the structure can be reviewed
//! before implementation.

use crate::planner::core::{PlanResult, PlannerConfig, PlannerError};
use crate::sim::GraphState;
use crate::units::{UnitKind, Units};

pub mod features;
pub mod search;
pub mod value_net;

pub use value_net::ValueNet;

/// Run MCTS from `initial_state` toward `goal_id`.
///
/// This is the entry point used by [`crate::planner::Planner`]. It will
/// eventually:
///
/// 1. Build a `ValueNet` (or load a checkpoint).
/// 2. Run `MctsSearch` for `iterations` expansions.
/// 3. Extract the best action sequence and convert it into a `PlanResult`.
pub fn plan(
    _units: &Units,
    _initial_state: GraphState,
    _goal_id: &UnitKind,
    _iterations: usize,
    _config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    todo!("MCTS + value-net planner is not yet implemented")
}
