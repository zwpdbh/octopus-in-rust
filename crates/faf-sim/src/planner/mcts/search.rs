//! Monte Carlo Tree Search planner.
//!
//! UCT search over the six high-level directions. Each node stores a simulator
//! state. The direction-only policy network supplies prior probabilities over
//! legal directions; rollouts use the same network greedily to estimate leaf
//! values.

use crate::planner::core::{Goal, PlanResult, PlannerConfig, PlannerError};
use crate::planner::mcts::features::state_features_with_shortfall;
use crate::planner::mcts::heuristic::{direction_to_action, is_direction_legal};
use crate::planner::mcts::macro_net::{apply_mask, masked_argmax};
use crate::planner::mcts::policy::{execute_action, plan_result_with_action};
use crate::planner::mcts::train::reward::{compute_step_reward, compute_terminal_bonus};
use crate::planner::mcts::value_net::ValueNet;
use crate::planner::plan_graph::{build_plan_graph, EdgeCategory, PlanGraph};
use crate::planner::SimAction;
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
    /// Child nodes, keyed by direction index.
    children: Vec<(usize, Box<MctsNode>)>,
    /// Legal directions that have not been expanded yet.
    untried_directions: Vec<usize>,
    /// Prior probability for each direction (sparse: legal directions only).
    direction_priors: Vec<f32>,
    /// True if the state has reached the goal.
    is_terminal: bool,
}

impl MctsNode {
    /// Create a new node and compute direction priors from the policy network.
    fn new(
        state: SimulationState,
        goal: &Goal,
        units: &Units,
        config: &PlannerConfig,
        model: &dyn ValueNet,
        plan: &PlanGraph,
    ) -> Self {
        let is_terminal = state.goal_reached(goal);
        let (direction_priors, untried_directions) = if is_terminal {
            (vec![0.0f32; EdgeCategory::ALL.len()], Vec::new())
        } else {
            evaluate_direction_priors(&state, goal, units, config, model, plan)
        };

        Self {
            state,
            total_value: 0.0,
            visits: 0,
            children: Vec::new(),
            untried_directions,
            direction_priors,
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

    /// Run MCTS from `initial_state` toward `goal` and return the best plan.
    pub fn search(
        &self,
        initial_state: SimulationState,
        goal: &Goal,
        units: &Units,
        planner_config: &PlannerConfig,
        model: &dyn ValueNet,
    ) -> Result<PlanResult, PlannerError> {
        let plan = build_plan_graph(units, *goal);
        let mut root = MctsNode::new(initial_state, goal, units, planner_config, model, &plan);

        if root.untried_directions.is_empty() && !root.is_terminal {
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
            } else if leaf.untried_directions.is_empty() {
                // Fully expanded leaf: run a rollout from this state.
                rollout_value(
                    &leaf.state,
                    goal,
                    units,
                    planner_config,
                    model,
                    self.config.max_rollout_steps,
                    &plan,
                )
            } else {
                // Expand one untried direction, then rollout from the child.
                let direction_idx = leaf
                    .untried_directions
                    .iter()
                    .copied()
                    .max_by(|a, b| {
                        leaf.direction_priors[*a]
                            .partial_cmp(&leaf.direction_priors[*b])
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .unwrap_or(leaf.untried_directions[0]);

                leaf.untried_directions.retain(|&d| d != direction_idx);

                match expand_direction(
                    &leaf.state,
                    direction_idx,
                    goal,
                    units,
                    planner_config,
                    &plan,
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
                                self.config.max_rollout_steps,
                                &plan,
                            )
                        };
                        leaf.children.push((
                            direction_idx,
                            Box::new(MctsNode {
                                state: child_state,
                                total_value: child_value,
                                visits: 1,
                                children: Vec::new(),
                                untried_directions: Vec::new(),
                                direction_priors: vec![0.0f32; EdgeCategory::ALL.len()],
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
        let best_direction = root
            .children
            .iter()
            .max_by(|(_, a), (_, b)| a.visits.cmp(&b.visits))
            .map(|(direction_idx, _)| *direction_idx);

        let mut final_state = root.state.clone();
        let action = if let Some(direction_idx) = best_direction {
            let direction = EdgeCategory::ALL[direction_idx];
            let action =
                direction_to_action(direction, &root.state, units, planner_config, goal, &plan);
            if execute_action(&mut final_state, &action, units, planner_config.dt).is_ok() {
                action
            } else {
                SimAction::Wait
            }
        } else {
            SimAction::Wait
        };

        Ok(plan_result_with_action(final_state, action))
    }
}

/// Select a path from the root to a leaf using UCB1/PUCT.
fn select_path(node: &MctsNode, c_puct: f64) -> Vec<usize> {
    let mut path = Vec::new();
    let mut current = node;

    while !current.is_terminal
        && current.untried_directions.is_empty()
        && !current.children.is_empty()
    {
        let parent_visits = current.visits as f64;
        let best = current
            .children
            .iter()
            .enumerate()
            .map(|(i, (direction_idx, child))| {
                let prior = current.direction_priors[*direction_idx] as f64;
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
/// legal directions.
fn evaluate_direction_priors(
    state: &SimulationState,
    goal: &Goal,
    units: &Units,
    config: &PlannerConfig,
    model: &dyn ValueNet,
    plan: &PlanGraph,
) -> (Vec<f32>, Vec<usize>) {
    let shortfall = [0.0f32; 3];
    let features = state_features_with_shortfall(state, units, config, shortfall);

    let mut direction_logits = model.evaluate_direction(features);
    let direction_mask = legal_direction_mask(state, units, config, goal, plan);
    apply_mask(&mut direction_logits, &direction_mask);
    let direction_probs = softmax_probs(&direction_logits);

    let untried: Vec<usize> = direction_probs
        .iter()
        .enumerate()
        .filter(|(_, &p)| p > 0.0)
        .map(|(i, _)| i)
        .collect();

    (direction_probs, untried)
}

/// Build a boolean mask over [`EdgeCategory::ALL`] indicating which directions
/// have at least one legal concrete action right now.
fn legal_direction_mask(
    state: &SimulationState,
    units: &Units,
    config: &PlannerConfig,
    goal: &Goal,
    plan: &PlanGraph,
) -> Vec<bool> {
    EdgeCategory::ALL
        .iter()
        .map(|&d| is_direction_legal(d, state, units, config, goal, plan))
        .collect()
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

/// Apply a concrete direction to `state` and return the resulting state.
fn expand_direction(
    state: &SimulationState,
    direction_idx: usize,
    goal: &Goal,
    units: &Units,
    config: &PlannerConfig,
    plan: &PlanGraph,
) -> Option<SimulationState> {
    let direction = EdgeCategory::ALL.get(direction_idx)?;
    let action = direction_to_action(*direction, state, units, config, goal, plan);
    if matches!(action, SimAction::Wait) {
        return None;
    }

    let mut new_state = state.clone();
    if execute_action(&mut new_state, &action, units, config.dt).is_err() {
        return None;
    }

    Some(new_state)
}

/// Run a rollout from `state` using the direction-only policy and return the
/// discounted sum of step rewards plus a terminal bonus.
fn rollout_value(
    state: &SimulationState,
    goal: &Goal,
    units: &Units,
    config: &PlannerConfig,
    model: &dyn ValueNet,
    max_steps: usize,
    plan: &PlanGraph,
) -> f64 {
    let mut s = state.clone();
    let mut total = 0.0f32;
    let mut discount = 1.0f32;
    let gamma = 0.99f32;

    for _ in 0..max_steps {
        if s.goal_reached(goal) {
            break;
        }

        let prev = s.clone();

        let action = greedy_direction_action(&s, goal, units, config, model, plan);
        if execute_action(&mut s, &action, units, config.dt).is_err() {
            s.tick(units, config.dt);
        }

        total += discount * compute_step_reward(&prev, &s, units);
        discount *= gamma;
    }

    let terminal = compute_terminal_bonus(&s, s.goal_reached(goal));
    (total + discount * terminal) as f64
}

/// Greedily pick a direction using the policy network and heuristic layer.
fn greedy_direction_action(
    state: &SimulationState,
    goal: &Goal,
    units: &Units,
    config: &PlannerConfig,
    model: &dyn ValueNet,
    plan: &PlanGraph,
) -> SimAction {
    let shortfall = [0.0f32; 3];
    let features = state_features_with_shortfall(state, units, config, shortfall);
    let direction_logits = model.evaluate_direction(features);
    let direction_mask = legal_direction_mask(state, units, config, goal, plan);

    if let Some(direction_idx) = masked_argmax(&direction_logits, &direction_mask) {
        let direction = EdgeCategory::ALL[direction_idx];
        return direction_to_action(direction, state, units, config, goal, plan);
    }

    SimAction::Wait
}
