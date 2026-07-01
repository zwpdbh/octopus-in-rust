//! Trainer for the hierarchical policy networks.

use std::time::{Duration, Instant};

use rand::RngExt;

use super::super::episode::{Episode, EpisodeStep};
use super::super::math::format_time;
use super::super::reward::{compute_step_reward, compute_terminal_bonus, MilestoneTracker};

use crate::planner::core::{Goal, PlannerConfig};
use crate::planner::mcts::features::{state_features, state_features_with_shortfall};
use crate::planner::mcts::macro_net::{
    clamp_squad, ensure_minimum_squad, masked_sample_index, shortfall_from_counts, DIRECTION_COUNT,
};
use crate::planner::mcts::policy::{
    execute_action, find_upgrade_edge_idx, upgrade_mask, FACTORY_UPGRADE_OPTIONS,
};
use crate::planner::mcts::selections::{
    assigned_squad_counts, find_upgrade_source, idle_engineer_counts, select_squad_for_edge,
    PlanEdgeIndex,
};
use crate::planner::plan_graph::{EdgeCategory, PlanEdgeKind, PlanGraph};
use crate::planner::search::SimAction;
use crate::sim::{GraphState, NodeId};
use crate::units::{UnitKind, Units};

use super::Trainer;

struct StepDecision {
    direction_mask: Vec<bool>,
    direction_index: usize,
    action_mask: Vec<bool>,
    edge_index: usize,
    target_power: f32,
    sampled_squad: [f32; 3],
    desired: [usize; 3],
    available: [usize; 3],
    builders: Vec<NodeId>,
    action: SimAction,
}

impl Trainer {
    /// Run one episode and record the trajectory.
    pub(crate) fn run_episode(
        &mut self,
        ep: usize,
        units: &Units,
        goal: &Goal,
        _plan: &PlanGraph,
        edge_index: &PlanEdgeIndex,
        planner_config: &PlannerConfig,
        epsilon: f32,
    ) -> Episode {
        let mut state = GraphState::new(units, &[UnitKind::Commander]);
        let mut episode = Episode {
            reached_goal: false,
            completion_time: 0.0,
            final_reward: 0.0,
            steps: Vec::new(),
        };
        let mut shortfall = [0.0f32; 3];
        let mut milestones = MilestoneTracker::default();

        let progress_interval = Duration::from_secs(2);
        let mut last_progress = Instant::now();

        for step in 0..self.config.max_steps {
            if self.config.verbose && last_progress.elapsed() >= progress_interval {
                eprintln!(
                    "  progress: ep={:>4} step={:>5} sim_time={:>12}",
                    ep + 1,
                    step,
                    format_time(state.time, true)
                );
                last_progress = Instant::now();
            }

            if state.goal_reached(goal) {
                episode.reached_goal = true;
                episode.completion_time = state.time;
                break;
            }

            let base_features = state_features(&state, units, planner_config);
            let macro_features =
                state_features_with_shortfall(&state, units, planner_config, shortfall);

            // The upgrade head decides whether to tech up a factory before the
            // normal direction/action pipeline runs.
            let upgrade_legal_mask = upgrade_mask(edge_index, &state, units, planner_config);
            let upgrade_logits = self
                .model
                .evaluate_upgrade(macro_features.clone(), &self.device);
            let upgrade_idx = if self.rng.random::<f32>() < epsilon {
                let legal_upgrades: Vec<usize> = upgrade_legal_mask
                    .iter()
                    .enumerate()
                    .filter(|(_, &legal)| legal)
                    .map(|(i, _)| i)
                    .collect();
                *legal_upgrades
                    .get(self.rng.random_range(0..legal_upgrades.len()))
                    .unwrap_or(&0)
            } else {
                masked_sample_index(&upgrade_logits, &upgrade_legal_mask, &mut self.rng)
                    .unwrap_or(0)
            };

            let decision = if upgrade_idx > 0 {
                let (source_kind, target_kind) = &FACTORY_UPGRADE_OPTIONS[upgrade_idx - 1];
                let upgrade_edge_idx =
                    match find_upgrade_edge_idx(edge_index, source_kind, target_kind) {
                        Some(idx) => idx,
                        None => {
                            state.tick(units, self.config.dt);
                            continue;
                        }
                    };
                let edge = match edge_index.get(upgrade_edge_idx) {
                    Some(e) => e.clone(),
                    None => {
                        state.tick(units, self.config.dt);
                        continue;
                    }
                };

                let power_mean = self.model.evaluate_power(
                    macro_features.clone(),
                    upgrade_edge_idx,
                    edge_index.len(),
                    &self.device,
                );
                let target_power = crate::planner::mcts::macro_net::sample_gaussian(
                    power_mean,
                    self.config.power_std,
                    &mut self.rng,
                )
                .max(0.0)
                .round();

                let squad_raw =
                    self.model
                        .evaluate_squad(macro_features, target_power, &self.device);
                let squad_raw_arr = [
                    squad_raw.get(0).copied().unwrap_or(0.0),
                    squad_raw.get(1).copied().unwrap_or(0.0),
                    squad_raw.get(2).copied().unwrap_or(0.0),
                ];
                let sampled_squad = [
                    crate::planner::mcts::macro_net::sample_gaussian(
                        squad_raw_arr[0],
                        self.config.squad_std,
                        &mut self.rng,
                    )
                    .max(0.0),
                    crate::planner::mcts::macro_net::sample_gaussian(
                        squad_raw_arr[1],
                        self.config.squad_std,
                        &mut self.rng,
                    )
                    .max(0.0),
                    crate::planner::mcts::macro_net::sample_gaussian(
                        squad_raw_arr[2],
                        self.config.squad_std,
                        &mut self.rng,
                    )
                    .max(0.0),
                ];

                let available = idle_engineer_counts(&state, units);
                let mut desired = clamp_squad(sampled_squad, available);
                desired = ensure_minimum_squad(desired, available);

                let builders = select_squad_for_edge(&edge, desired, &state, units);
                if builders.is_empty() {
                    shortfall = shortfall_from_counts(desired, available);
                    state.tick(units, self.config.dt);
                    continue;
                }

                let old_node = find_upgrade_source(&state, source_kind)
                    .unwrap_or_else(|| crate::sim::NodeId::new(0));
                let action = SimAction::Upgrade {
                    target_unit_id: target_kind.clone(),
                    old_node,
                    builders: builders.clone(),
                };

                StepDecision {
                    direction_mask: vec![false; DIRECTION_COUNT],
                    direction_index: 0,
                    action_mask: vec![false; edge_index.len()],
                    edge_index: upgrade_edge_idx,
                    target_power,
                    sampled_squad,
                    desired,
                    available,
                    builders,
                    action,
                }
            } else {
                let direction_mask = edge_index.legal_category_mask(&state, units, planner_config);
                if direction_mask.iter().all(|&b| !b) {
                    state.tick(units, self.config.dt);
                    continue;
                }

                let direction_logits = self
                    .model
                    .evaluate_direction(macro_features.clone(), &self.device);

                let (direction_idx, category) = if self.rng.random::<f32>() < epsilon {
                    let legal_directions: Vec<usize> = direction_mask
                        .iter()
                        .enumerate()
                        .filter(|(_, &legal)| legal)
                        .map(|(i, _)| i)
                        .collect();
                    let idx = *legal_directions
                        .get(self.rng.random_range(0..legal_directions.len()))
                        .unwrap_or(&0);
                    (idx, EdgeCategory::ALL[idx])
                } else {
                    let idx =
                        masked_sample_index(&direction_logits, &direction_mask, &mut self.rng)
                            .unwrap_or(0);
                    (idx, EdgeCategory::ALL[idx])
                };

                let action_mask =
                    edge_index.legal_mask_for_category(&state, units, planner_config, category);
                if action_mask.iter().all(|&b| !b) {
                    state.tick(units, self.config.dt);
                    continue;
                }

                let action_logits =
                    self.model
                        .evaluate_action(macro_features.clone(), category, &self.device);

                let edge_idx = if self.rng.random::<f32>() < epsilon {
                    let legal_indices: Vec<usize> = action_mask
                        .iter()
                        .enumerate()
                        .filter(|(_, &legal)| legal)
                        .map(|(i, _)| i)
                        .collect();
                    *legal_indices
                        .get(self.rng.random_range(0..legal_indices.len()))
                        .unwrap_or(&0)
                } else {
                    masked_sample_index(&action_logits, &action_mask, &mut self.rng).unwrap_or(0)
                };

                let edge = match edge_index.get(edge_idx) {
                    Some(e) => e.clone(),
                    None => {
                        state.tick(units, self.config.dt);
                        continue;
                    }
                };

                let power_mean = self.model.evaluate_power(
                    macro_features.clone(),
                    edge_idx,
                    edge_index.len(),
                    &self.device,
                );
                let target_power = crate::planner::mcts::macro_net::sample_gaussian(
                    power_mean,
                    self.config.power_std,
                    &mut self.rng,
                )
                .max(0.0)
                .round();

                let squad_raw =
                    self.model
                        .evaluate_squad(macro_features, target_power, &self.device);
                let squad_raw_arr = [
                    squad_raw.get(0).copied().unwrap_or(0.0),
                    squad_raw.get(1).copied().unwrap_or(0.0),
                    squad_raw.get(2).copied().unwrap_or(0.0),
                ];
                let sampled_squad = [
                    crate::planner::mcts::macro_net::sample_gaussian(
                        squad_raw_arr[0],
                        self.config.squad_std,
                        &mut self.rng,
                    )
                    .max(0.0),
                    crate::planner::mcts::macro_net::sample_gaussian(
                        squad_raw_arr[1],
                        self.config.squad_std,
                        &mut self.rng,
                    )
                    .max(0.0),
                    crate::planner::mcts::macro_net::sample_gaussian(
                        squad_raw_arr[2],
                        self.config.squad_std,
                        &mut self.rng,
                    )
                    .max(0.0),
                ];

                let available = idle_engineer_counts(&state, units);
                let mut desired = clamp_squad(sampled_squad, available);
                desired = ensure_minimum_squad(desired, available);

                let builders = select_squad_for_edge(&edge, desired, &state, units);
                if builders.is_empty() {
                    shortfall = shortfall_from_counts(desired, available);
                    state.tick(units, self.config.dt);
                    continue;
                }

                let action = match edge.kind {
                    PlanEdgeKind::Build => {
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
                    PlanEdgeKind::Upgrade => SimAction::Upgrade {
                        target_unit_id: edge.target_unit().expect("upgrade target unit").clone(),
                        old_node: find_upgrade_source(
                            &state,
                            edge.source_unit().expect("upgrade source unit"),
                        )
                        .unwrap_or_else(|| crate::sim::NodeId::new(0)),
                        builders: builders.clone(),
                    },
                };

                StepDecision {
                    direction_mask,
                    direction_index: direction_idx,
                    action_mask,
                    edge_index: edge_idx,
                    target_power,
                    sampled_squad,
                    desired,
                    available,
                    builders,
                    action,
                }
            };

            let prev_state = state.clone();
            if execute_action(&mut state, &decision.action, units, self.config.dt).is_err() {
                shortfall = shortfall_from_counts(decision.desired, decision.available);
                state.tick(units, self.config.dt);
                continue;
            }

            let mut step_reward = compute_step_reward(&prev_state, &state, units);
            step_reward += milestones.update(&state, units);

            episode.steps.push(EpisodeStep {
                base_features,
                shortfall,
                upgrade_mask: upgrade_legal_mask,
                upgrade_index: upgrade_idx,
                direction_mask: decision.direction_mask,
                direction_index: decision.direction_index,
                action_mask: decision.action_mask,
                edge_index: decision.edge_index,
                target_power: decision.target_power,
                desired_squad: decision.sampled_squad,
                step_reward,
                return_value: 0.0,
            });

            let assigned_counts = assigned_squad_counts(&state, &decision.builders);
            shortfall = shortfall_from_counts(decision.desired, assigned_counts);
        }

        episode.final_reward = compute_terminal_bonus(&state, episode.reached_goal);
        self.compute_returns(&mut episode);
        episode
    }
}
