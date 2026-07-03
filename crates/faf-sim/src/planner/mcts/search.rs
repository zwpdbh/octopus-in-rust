//! Monte Carlo Tree Search planner.
//!
//! UCT search over plan-graph actions. Each node stores a simulator state. The
//! hierarchical policy network supplies prior probabilities over legal edges;
//! rollouts use the same network greedily to estimate leaf values.

use std::collections::HashSet;

use crate::planner::core::{Goal, PlanResult, PlannerConfig, PlannerError};
use crate::planner::mcts::features::state_features_with_shortfall;
use crate::planner::mcts::macro_net::{apply_mask, clamp_squad, ensure_minimum_squad};
use crate::planner::mcts::policy::{execute_action, macro_policy_plan};
use crate::planner::mcts::selections::{
    find_upgrade_source, idle_engineer_counts, select_squad_for_edge, PlanEdgeIndex,
};
use crate::planner::mcts::train::reward::{compute_step_reward, compute_terminal_bonus};
use crate::planner::mcts::value_net::ValueNet;
use crate::planner::plan_graph::EdgeCategory;
use crate::planner::search::SimAction;
use crate::sim::SimulationState;
use crate::units::Units;

/// Configuration for an MCTS search.
#[derive(Debug, Clone, Copy)]
pub struct MctsConfig {
    /// Number of MCTS iterations (selection/expansion/evaluation/backup loops).
    pub iterations: usize,
    /// UCT exploration constant.
    pub c_puct: f64,
    /// Maximum rollout length in simulator steps.
    pub max_rollout_steps: usize,
}

impl Default for MctsConfig {
    fn default() -> Self {
        Self {
            iterations: 200,
            c_puct: 1.414,
            max_rollout_steps: 500,
        }
    }
}

/// A node in the MCTS tree.
struct MctsNode {
    /// Simulator state at this node.
    state: SimulationState,
    /// Total value accumulated from backpropagation.
    total_value: f64,
    /// Number of times this node has been visited.
    visits: usize,
    /// Child nodes, keyed by the plan-graph edge used to reach them.
    children: Vec<(usize, Box<MctsNode>)>,
    /// Legal edges that have not been expanded yet.
    untried_edges: Vec<usize>,
    /// Prior probability for each plan-graph edge (sparse: legal edges only).
    edge_priors: Vec<f32>,
    /// True if the state has reached the goal.
    is_terminal: bool,
}

impl MctsNode {
    /// Create a new node and compute edge priors from the policy network.
    fn new(
        state: SimulationState,
        goal: &Goal,
        units: &Units,
        config: &PlannerConfig,
        edge_index: &PlanEdgeIndex,
        model: &dyn ValueNet,
    ) -> Self {
        let is_terminal = state.goal_reached(goal);
        let (edge_priors, untried_edges) = if is_terminal {
            (vec![0.0f32; edge_index.len()], Vec::new())
        } else {
            evaluate_edge_priors(&state, units, config, edge_index, model)
        };

        Self {
            state,
            total_value: 0.0,
            visits: 0,
            children: Vec::new(),
            untried_edges,
            edge_priors,
            is_terminal,
        }
    }
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

    /// Run MCTS from `initial_state` toward `goal_id` and return the best plan.
    ///
    /// # Arguments
    ///
    /// * `initial_state` - The current simulator state (root of the tree).
    /// * `goal` - The abstract target being planned or trained for.
    /// * `units` - Unified unit knowledge repository.
    /// * `planner_config` - Shared planner configuration.
    /// * `model` - The learned hierarchical policy used for priors and rollouts.
    pub fn search(
        &self,
        initial_state: SimulationState,
        goal: &Goal,
        units: &Units,
        planner_config: &PlannerConfig,
        model: &dyn ValueNet,
    ) -> Result<PlanResult, PlannerError> {
        let edge_index = PlanEdgeIndex::new(&units.plan_graph(*goal));

        let mut root = MctsNode::new(
            initial_state,
            goal,
            units,
            planner_config,
            &edge_index,
            model,
        );

        if root.untried_edges.is_empty() && !root.is_terminal {
            // No legal actions from the root.
            let mut state = root.state.clone();
            state.tick(units, planner_config.dt);
            return Ok(plan_result_with_action(state, SimAction::Wait));
        }

        for _ in 0..self.config.iterations {
            let path = select_path(&root, self.config.c_puct);

            // Walk to the selected leaf.
            let mut leaf = &mut root;
            for &child_idx in &path {
                leaf = &mut leaf.children[child_idx].1;
            }

            let value = if leaf.is_terminal {
                compute_terminal_bonus(&leaf.state, true) as f64
            } else if leaf.untried_edges.is_empty() {
                // Fully expanded leaf: run a rollout from this state.
                rollout_value(
                    &leaf.state,
                    goal,
                    units,
                    planner_config,
                    model,
                    &edge_index,
                    self.config.max_rollout_steps,
                )
            } else {
                // Expand one untried edge, then rollout from the child.
                let edge_idx = leaf
                    .untried_edges
                    .iter()
                    .copied()
                    .max_by(|a, b| {
                        leaf.edge_priors[*a]
                            .partial_cmp(&leaf.edge_priors[*b])
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .unwrap_or(leaf.untried_edges[0]);

                leaf.untried_edges.retain(|&e| e != edge_idx);

                match expand_edge(
                    &leaf.state,
                    edge_idx,
                    goal,
                    units,
                    planner_config,
                    &edge_index,
                    model,
                ) {
                    Some(child_state) => {
                        let child_value = if child_state.goal_reached(goal) {
                            compute_terminal_bonus(&child_state, true) as f64
                        } else {
                            rollout_value(
                                &child_state,
                                goal,
                                units,
                                planner_config,
                                model,
                                &edge_index,
                                self.config.max_rollout_steps,
                            )
                        };
                        leaf.children.push((
                            edge_idx,
                            Box::new(MctsNode {
                                state: child_state,
                                total_value: child_value,
                                visits: 1,
                                children: Vec::new(),
                                untried_edges: Vec::new(),
                                edge_priors: vec![0.0f32; edge_index.len()],
                                is_terminal: true,
                            }),
                        ));
                        child_value
                    }
                    None => {
                        // Expansion failed (e.g. no builders available). Treat
                        // as a neutral rollout result.
                        0.0
                    }
                }
            };

            // Backup: update the leaf and every node on the path.
            leaf.total_value += value;
            leaf.visits += 1;
            let mut current = &mut root;
            current.total_value += value;
            current.visits += 1;
            for &child_idx in &path {
                current = &mut current.children[child_idx].1;
                current.total_value += value;
                current.visits += 1;
            }
        }

        // Pick the root child with the highest visit count.
        let best_edge = root
            .children
            .iter()
            .max_by(|(_, a), (_, b)| a.visits.cmp(&b.visits))
            .map(|(edge_idx, _)| *edge_idx);

        let mut final_state = root.state.clone();
        let action = if let Some(edge_idx) = best_edge {
            match expand_edge(
                &root.state,
                edge_idx,
                goal,
                units,
                planner_config,
                &edge_index,
                model,
            ) {
                Some(s) => {
                    final_state = s;
                    infer_action_from_states(&root.state, &final_state, edge_idx, &edge_index)
                }
                None => SimAction::Wait,
            }
        } else {
            SimAction::Wait
        };

        Ok(plan_result_with_action(final_state, action))
    }
}

/// Select a path from the root to a leaf using UCB1/PUCT.
///
/// Returns the indices of the child chosen at each level. The leaf is the node
/// reached after following all indices.
fn select_path(node: &MctsNode, c_puct: f64) -> Vec<usize> {
    let mut path = Vec::new();
    let mut current = node;

    while !current.is_terminal && current.untried_edges.is_empty() && !current.children.is_empty() {
        let parent_visits = current.visits as f64;
        let best = current
            .children
            .iter()
            .enumerate()
            .map(|(i, (edge_idx, child))| {
                let prior = current.edge_priors[*edge_idx] as f64;
                let q = if child.visits == 0 {
                    0.0
                } else {
                    child.total_value / child.visits as f64
                };
                let u = c_puct * prior * parent_visits.sqrt() / (1.0 + child.visits as f64);
                (i, q + u)
            })
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(0);

        path.push(best);
        current = &current.children[best].1;
    }

    path
}

/// Evaluate the policy network at `state` and return prior probabilities over
/// legal plan-graph edges.
fn evaluate_edge_priors(
    state: &SimulationState,
    units: &Units,
    config: &PlannerConfig,
    edge_index: &PlanEdgeIndex,
    model: &dyn ValueNet,
) -> (Vec<f32>, Vec<usize>) {
    let shortfall = [0.0f32; 3];
    let features = state_features_with_shortfall(state, units, config, shortfall);

    let mut direction_logits = model.evaluate_direction(features.clone());
    let direction_mask = edge_index.legal_category_mask(state, units);
    apply_mask(&mut direction_logits, &direction_mask);
    let direction_probs = softmax_probs(&direction_logits);

    let mut edge_prior = vec![0.0f32; edge_index.len()];
    for (d, &dp) in direction_probs.iter().enumerate() {
        if dp <= 0.0 {
            continue;
        }
        let category = EdgeCategory::ALL[d];
        let mut action_logits = model.evaluate_action(features.clone(), category);
        let action_mask = edge_index.legal_mask_for_category(state, units, category);
        apply_mask(&mut action_logits, &action_mask);
        let action_probs = softmax_probs(&action_logits);
        for (edge_idx, &p) in action_probs.iter().enumerate() {
            if p > 0.0 {
                edge_prior[edge_idx] += dp * p;
            }
        }
    }

    let total: f32 = edge_prior.iter().sum();
    if total > 0.0 {
        for p in &mut edge_prior {
            *p /= total;
        }
    }

    let untried: Vec<usize> = edge_prior
        .iter()
        .enumerate()
        .filter(|(_, &p)| p > 0.0)
        .map(|(i, _)| i)
        .collect();

    (edge_prior, untried)
}

/// Compute a softmax probability vector from masked logits.
fn softmax_probs(logits: &[f32]) -> Vec<f32> {
    if logits.is_empty() {
        return Vec::new();
    }
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exps: Vec<f32> = logits.iter().map(|&s| (s - max).exp()).collect();
    let sum: f32 = exps.iter().sum();
    if sum <= 0.0 || !sum.is_finite() {
        return vec![1.0f32 / logits.len() as f32; logits.len()];
    }
    exps.iter().map(|&e| e / sum).collect()
}

/// Apply a concrete plan-graph edge to `state` and return the resulting state.
fn expand_edge(
    state: &SimulationState,
    edge_idx: usize,
    _goal: &Goal,
    units: &Units,
    config: &PlannerConfig,
    edge_index: &PlanEdgeIndex,
    model: &dyn ValueNet,
) -> Option<SimulationState> {
    let edge = edge_index.get(edge_idx)?;

    let shortfall = [0.0f32; 3];
    let features = state_features_with_shortfall(state, units, config, shortfall);
    let power_mean = model.evaluate_power(features.clone(), edge_idx, edge_index.len());
    let target_power = power_mean.max(0.0).round();

    let squad_raw = model.evaluate_squad(features, target_power);
    let squad_raw_arr = [
        squad_raw.get(0).copied().unwrap_or(0.0),
        squad_raw.get(1).copied().unwrap_or(0.0),
        squad_raw.get(2).copied().unwrap_or(0.0),
    ];

    let available = idle_engineer_counts(state, units);
    let desired = ensure_minimum_squad(clamp_squad(squad_raw_arr, available), available);
    let builders = select_squad_for_edge(edge, desired, state, units);
    if builders.is_empty() {
        return None;
    }

    let action = match edge.kind {
        crate::planner::plan_graph::EdgeAction::Build => {
            if let Some(target_goal) = edge.target_goal() {
                SimAction::BuildGoal {
                    goal: *target_goal,
                    builders: builders.clone(),
                }
            } else {
                SimAction::Build {
                    unit_id: edge.target_unit().expect("build target unit").clone(),
                    builders: builders.clone(),
                }
            }
        }
        crate::planner::plan_graph::EdgeAction::Upgrade => SimAction::Upgrade {
            target_unit_id: edge.target_unit().expect("upgrade target unit").clone(),
            old_node: find_upgrade_source(state, edge.source_unit().expect("upgrade source unit"))
                .unwrap_or_else(|| crate::sim::NodeId::new(0)),
            builders: builders.clone(),
        },
    };

    let mut new_state = state.clone();
    if execute_action(&mut new_state, &action, units, config.dt).is_err() {
        return None;
    }

    Some(new_state)
}

/// Run a rollout from `state` using the hierarchical policy and return the
/// discounted sum of step rewards plus a terminal bonus.
fn rollout_value(
    state: &SimulationState,
    goal: &Goal,
    units: &Units,
    config: &PlannerConfig,
    model: &dyn ValueNet,
    _edge_index: &PlanEdgeIndex,
    max_steps: usize,
) -> f64 {
    let mut s = state.clone();
    let mut total = 0.0f32;
    let mut discount = 1.0f32;
    let gamma = 0.99f32;
    let mut shortfall = [0.0f32; 3];

    for _ in 0..max_steps {
        if s.goal_reached(goal) {
            break;
        }

        let prev = s.clone();
        let result = macro_policy_plan(
            units,
            s.clone(),
            goal,
            Some(model),
            true,
            &mut shortfall,
            config,
        );

        match result {
            Ok(plan_result) => {
                let action = plan_result.first_action.unwrap_or(SimAction::Wait);
                if execute_action(&mut s, &action, units, config.dt).is_err() {
                    s.tick(units, config.dt);
                }
            }
            Err(_) => {
                s.tick(units, config.dt);
            }
        }

        total += discount * compute_step_reward(&prev, &s, units);
        discount *= gamma;
    }

    let terminal = compute_terminal_bonus(&s, s.goal_reached(goal));
    (total + discount * terminal) as f64
}

/// Reconstruct the `SimAction` that transformed `before` into `after`.
///
/// The MCTS node stores the state after applying the best edge. This helper
/// returns the corresponding action so the caller can build a `PlanResult`.
fn infer_action_from_states(
    before: &SimulationState,
    after: &SimulationState,
    edge_idx: usize,
    edge_index: &PlanEdgeIndex,
) -> SimAction {
    let Some(edge) = edge_index.get(edge_idx) else {
        return SimAction::Wait;
    };

    if let Some(goal) = edge.target_goal() {
        if !before.goal_project_active() && after.goal_project_active() {
            let builders = after
                .goal_project
                .as_ref()
                .map(|gp| gp.started_by.clone())
                .unwrap_or_default();
            return SimAction::BuildGoal {
                goal: *goal,
                builders,
            };
        }
        return SimAction::Wait;
    }

    let Some(target_unit) = edge.target_unit() else {
        return SimAction::Wait;
    };

    let before_active: HashSet<_> = before
        .graph
        .graph
        .node_weights()
        .filter(|n| n.is_active())
        .map(|n| n.id)
        .collect();

    // Find a newly active node that matches the edge target.
    for node in after.graph.graph.node_weights() {
        if node.is_active() && !before_active.contains(&node.id) {
            if node.unit_id == *target_unit {
                return SimAction::Build {
                    unit_id: target_unit.clone(),
                    builders: Vec::new(),
                };
            }
        }
    }

    SimAction::Wait
}

/// Build a [`PlanResult`] that commits to a single immediate action.
fn plan_result_with_action(state: SimulationState, action: SimAction) -> PlanResult {
    PlanResult {
        events: Vec::new(),
        completion_time: state.time,
        final_economy: state.economy,
        first_action: Some(action),
    }
}
