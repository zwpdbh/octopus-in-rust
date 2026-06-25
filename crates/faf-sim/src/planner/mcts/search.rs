//! Monte Carlo Tree Search core.
//!
//! Implements UCT selection, expansion, leaf evaluation with the value network,
//! and value backup. This scaffold declares the structure but leaves the
//! algorithm details as `todo!()`.

use faf_units::{DataIndex, Unit};

use crate::planner::core::{PlanResult, PlannerConfig, PlannerError};
use crate::sim::GraphState;

use super::ValueNet;

/// Configuration for an MCTS search.
#[derive(Debug, Clone, Copy)]
pub struct MctsConfig {
    /// Number of MCTS iterations (selection/expansion/evaluation/backup loops).
    pub iterations: usize,
    /// UCT exploration constant.
    pub c_puct: f64,
}

/// A node in the MCTS tree.
#[derive(Debug)]
pub struct MctsNode {
    /// Simulator state at this node.
    pub state: GraphState,
    /// Total value accumulated from backpropagation.
    pub total_value: f64,
    /// Number of times this node has been visited.
    pub visits: usize,
    /// Child nodes.
    pub children: Vec<MctsNode>,
}

/// MCTS search state.
#[derive(Debug)]
pub struct MctsSearch {
    config: MctsConfig,
}

impl MctsSearch {
    /// Create a new search with the given configuration.
    pub fn new(config: MctsConfig) -> Self {
        Self { config }
    }

    /// Run MCTS from `initial_state` toward `goal` and return the best plan.
    ///
    /// # Arguments
    ///
    /// * `initial_state` - The current simulator state (root of the tree).
    /// * `goal` - The unit we are trying to build.
    /// * `index` - Static unit blueprint data.
    /// * `planner_config` - Shared planner configuration.
    /// * `value_net` - The learned value network used for leaf evaluation.
    pub fn search(
        &self,
        _initial_state: GraphState,
        _goal: &Unit,
        _index: &DataIndex,
        _planner_config: &PlannerConfig,
        _value_net: &ValueNet,
    ) -> Result<PlanResult, PlannerError> {
        let _ = self.config;
        todo!("MCTS search loop is not yet implemented")
    }
}
