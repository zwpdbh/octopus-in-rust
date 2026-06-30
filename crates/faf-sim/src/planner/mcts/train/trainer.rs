//! Trainer for the hierarchical policy networks.

use std::f32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use burn::optim::adaptor::OptimizerAdaptor;
use burn::optim::{Adam, AdamConfig, Optimizer};
use burn::tensor::activation::log_softmax;
use burn::tensor::{Tensor, TensorData};
use rand::rngs::ThreadRng;
use rand::RngExt;

use super::config::TrainConfig;
use super::episode::{BuildTrajectory, Episode, EpisodeStep, TrajectoryStep};
use super::math::{
    format_time, gaussian_log_prob_scalar, gaussian_log_prob_vec, tensor1d_from_vec,
};
use super::observer::{EpisodeSummary, GreedyEvalSummary, TrainingObserver};
use super::reward::{compute_step_reward, compute_terminal_bonus};
use super::{TrainBackend, TrainDevice};
use crate::planner::core::PlannerConfig;
use crate::planner::mcts::features::{state_features, state_features_with_shortfall};
use crate::planner::mcts::macro_net::{
    clamp_squad, ensure_minimum_squad, masked_argmax, masked_sample_index, one_hot,
    shortfall_from_counts, PolicyBundle, DIRECTION_COUNT, MASK_VALUE,
};
use crate::planner::mcts::policy::execute_action;
use crate::planner::mcts::selections::{
    assigned_squad_counts, find_upgrade_source, idle_engineer_counts, select_squad_for_edge,
    PlanEdgeIndex,
};
use crate::planner::plan_graph::EdgeCategory;
use crate::planner::plan_graph::PlanGraph;
use crate::planner::search::SimAction;
use crate::sim::GraphState;
use crate::units::{UnitKind, Units};

/// Concrete optimizer type returned by `AdamConfig::init` for a full policy bundle.
type AdamOptimizer = OptimizerAdaptor<Adam, PolicyBundle<TrainBackend>, TrainBackend>;

pub struct Trainer {
    pub(crate) model: PolicyBundle<TrainBackend>,
    pub(crate) best_model: Option<PolicyBundle<TrainBackend>>,
    pub(crate) best_trajectory: Option<BuildTrajectory>,
    pub(crate) optimizer: AdamOptimizer,
    pub(crate) config: TrainConfig,
    pub(crate) device: TrainDevice,
    pub(crate) rng: ThreadRng,
    /// Optional progress observer. Kept `pub(crate)` so that higher-level entry
    /// points can move it to a fresh `Trainer` during supervised fine-tuning.
    pub(crate) observer: Option<Box<dyn TrainingObserver>>,
    /// Shared flag that can be set from another thread to request a graceful
    /// stop at the next episode boundary.
    pub(crate) stop_requested: Arc<AtomicBool>,
}

impl Trainer {
    /// Create a new trainer with random initialization.
    pub fn new(config: TrainConfig, num_edges: usize) -> Self {
        let device: TrainDevice = Default::default();
        let model = PolicyBundle::new(&device, num_edges);
        Self::from_model(config, model)
    }

    /// Create a trainer that continues from an existing model.
    pub fn from_model(config: TrainConfig, model: PolicyBundle<TrainBackend>) -> Self {
        let device: TrainDevice = Default::default();
        let optimizer = AdamConfig::new().init();
        Self {
            model,
            best_model: None,
            best_trajectory: None,
            optimizer,
            config,
            device,
            rng: rand::rng(),
            observer: None,
            stop_requested: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Attach a progress observer.
    pub fn with_observer(mut self, observer: impl TrainingObserver + 'static) -> Self {
        self.observer = Some(Box::new(observer));
        self
    }

    /// Request a graceful stop at the next episode boundary.
    pub fn request_stop(&self) {
        self.stop_requested.store(true, Ordering::Relaxed);
    }

    fn should_stop(&self) -> bool {
        self.stop_requested.load(Ordering::Relaxed)
            || self.observer.as_ref().map_or(false, |o| o.should_stop())
    }

    /// Train the policy on the given goal.
    pub fn train(&mut self, units: &Units, goal: &UnitKind) -> super::config::TrainStats {
        use super::config::TrainStats;

        let plan = units
            .plan_graph(goal)
            .expect("goal must be reachable for training");
        let edge_index = PlanEdgeIndex::new(&plan);
        let planner_config = PlannerConfig::default();
        let mut stats = TrainStats::default();

        let mut best_time: Option<f64> = if let Some(ref model) = self.best_model {
            let baseline = Trainer::evaluate_greedy_with_model(
                model,
                units,
                goal,
                &plan,
                &edge_index,
                &planner_config,
                self.config.max_steps,
                self.config.dt,
                &self.device,
            );
            if self.config.verbose {
                if let Some(t) = baseline {
                    eprintln!("Resumed model greedy baseline: {}", format_time(t, true));
                } else {
                    eprintln!("Resumed model did not reach the goal in a greedy evaluation.");
                }
            }
            baseline
        } else {
            None
        };

        let mut ep = 0usize;
        let mut episodes_since_best = 0usize;
        loop {
            if self.config.episodes != 0 && ep >= self.config.episodes {
                break;
            }

            if let Some(patience) = self.config.patience {
                if best_time.is_some() && episodes_since_best >= patience {
                    if self.config.verbose {
                        eprintln!("No improvement for {} episodes; stopping early.", patience);
                    }
                    break;
                }
            }

            let epsilon = self.current_epsilon(ep);
            let episode = self.run_episode(
                ep,
                units,
                goal,
                &plan,
                &edge_index,
                &planner_config,
                epsilon,
            );

            let loss = if !episode.steps.is_empty() {
                let loss = self.update(&episode);
                stats.losses.push(loss);
                Some(loss)
            } else {
                None
            };

            stats.episode_lengths.push(episode.steps.len());
            let mut target_hit = false;
            if episode.reached_goal {
                stats.goal_reaches += 1;
                stats.completion_times.push(episode.completion_time);
                let is_new_best = best_time.map_or(true, |t| episode.completion_time < t);
                if is_new_best {
                    best_time = Some(episode.completion_time);
                    episodes_since_best = 0;
                    self.best_model = Some(self.model.clone());
                    self.best_trajectory = Some(BuildTrajectory {
                        steps: episode
                            .steps
                            .iter()
                            .map(|s| TrajectoryStep {
                                edge_index: s.edge_index,
                                target_power: s.target_power,
                                desired_squad: s.desired_squad,
                                shortfall: s.shortfall,
                            })
                            .collect(),
                    });
                }
                if let Some(target) = self.config.target_time {
                    if episode.completion_time <= target {
                        target_hit = true;
                    }
                }
            }

            let interval = self.config.greedy_eval_interval;
            if interval > 0 && ep > 0 && (ep + 1) % interval == 0 {
                if self.config.verbose {
                    eprintln!("  greedy eval at ep={}: running...", ep + 1);
                }
                if let Some(greedy_time) =
                    self.evaluate_greedy(units, goal, &plan, &edge_index, &planner_config)
                {
                    let is_new_best = best_time.map_or(true, |t| greedy_time < t);
                    if is_new_best {
                        best_time = Some(greedy_time);
                        episodes_since_best = 0;
                        self.best_model = Some(self.model.clone());
                        self.best_trajectory = None;
                    }
                    if self.config.verbose {
                        eprintln!(
                            "  greedy eval at ep={}: time={} best={}",
                            ep + 1,
                            format_time(greedy_time, true),
                            format_time(best_time.unwrap_or(0.0), best_time.is_some())
                        );
                    }
                    if let Some(ref mut observer) = self.observer {
                        observer.on_greedy_eval(GreedyEvalSummary {
                            episode: ep + 1,
                            reached_goal: true,
                            completion_time: Some(greedy_time),
                            best_time,
                        });
                    }
                } else {
                    if self.config.verbose {
                        eprintln!("  greedy eval at ep={}: did not reach goal", ep + 1);
                    }
                    if let Some(ref mut observer) = self.observer {
                        observer.on_greedy_eval(GreedyEvalSummary {
                            episode: ep + 1,
                            reached_goal: false,
                            completion_time: None,
                            best_time,
                        });
                    }
                }
            }

            if let Some(ref mut observer) = self.observer {
                observer.on_episode_end(EpisodeSummary {
                    episode: ep + 1,
                    total_episodes: self.config.episodes,
                    steps: episode.steps.len(),
                    epsilon,
                    reached_goal: episode.reached_goal,
                    completion_time: episode.completion_time,
                    loss,
                    best_time,
                });
                if self.should_stop() {
                    if self.config.verbose {
                        eprintln!("Stop requested; exiting training loop.");
                    }
                    break;
                }
            }

            if self.config.verbose {
                let time_str = format_time(episode.completion_time, episode.reached_goal);
                let best_str = format_time(best_time.unwrap_or(0.0), best_time.is_some());
                let loss_str = loss
                    .map(|l| format!("{:.4}", l))
                    .unwrap_or_else(|| "-".to_string());
                eprintln!(
                    "ep={:>4} steps={:>4} eps={:.4} reached={:>5} time={:>14} best={:>14} loss={:>10}",
                    ep + 1,
                    episode.steps.len(),
                    epsilon,
                    episode.reached_goal,
                    time_str,
                    best_str,
                    loss_str
                );
            }

            ep += 1;
            episodes_since_best += 1;

            if target_hit {
                if self.config.verbose {
                    eprintln!("Target completion time reached; stopping early.");
                }
                break;
            }
        }

        stats
    }

    /// Run one episode and record the trajectory.
    fn run_episode(
        &mut self,
        ep: usize,
        units: &Units,
        goal: &UnitKind,
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

            let direction_mask = edge_index.legal_category_mask(&state, units, planner_config);
            if direction_mask.iter().all(|&b| !b) {
                state.tick(units, self.config.dt);
                continue;
            }

            let base_features = state_features(&state, units, planner_config);
            let macro_features =
                state_features_with_shortfall(&state, units, planner_config, shortfall);
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
                let idx = masked_sample_index(&direction_logits, &direction_mask, &mut self.rng)
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

            let squad_raw = self
                .model
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
                crate::planner::plan_graph::PlanEdgeKind::Build => SimAction::Build {
                    unit_id: edge.target.clone(),
                    builders: builders.clone(),
                },
                crate::planner::plan_graph::PlanEdgeKind::Upgrade => SimAction::Upgrade {
                    target_unit_id: edge.target.clone(),
                    old_node: find_upgrade_source(&state, &edge.source)
                        .unwrap_or_else(|| crate::sim::NodeId::new(0)),
                    builders: builders.clone(),
                },
            };

            let prev_state = state.clone();
            if execute_action(&mut state, &action, units, self.config.dt).is_err() {
                shortfall = shortfall_from_counts(desired, available);
                state.tick(units, self.config.dt);
                continue;
            }

            let step_reward = compute_step_reward(&prev_state, &state, units);

            episode.steps.push(EpisodeStep {
                base_features,
                shortfall,
                direction_mask,
                direction_index: direction_idx,
                action_mask,
                edge_index: edge_idx,
                target_power,
                desired_squad: sampled_squad,
                step_reward,
                return_value: 0.0,
            });

            let assigned_counts = assigned_squad_counts(&state, &builders);
            shortfall = shortfall_from_counts(desired, assigned_counts);
        }

        episode.final_reward = compute_terminal_bonus(&state, episode.reached_goal);
        self.compute_returns(&mut episode);
        episode
    }

    fn current_epsilon(&self, ep: usize) -> f32 {
        let decay = self.config.epsilon_decay_episodes;
        if decay == 0 {
            return self.config.epsilon;
        }
        if ep >= decay {
            return self.config.epsilon_final;
        }
        let progress = ep as f32 / decay as f32;
        self.config.epsilon - (self.config.epsilon - self.config.epsilon_final) * progress
    }

    fn evaluate_greedy(
        &self,
        units: &Units,
        goal: &UnitKind,
        plan: &PlanGraph,
        edge_index: &PlanEdgeIndex,
        planner_config: &PlannerConfig,
    ) -> Option<f64> {
        Self::evaluate_greedy_with_model(
            &self.model,
            units,
            goal,
            plan,
            edge_index,
            planner_config,
            self.config.max_steps,
            self.config.dt,
            &self.device,
        )
    }

    fn evaluate_greedy_with_model(
        model: &PolicyBundle<TrainBackend>,
        units: &Units,
        goal: &UnitKind,
        _plan: &PlanGraph,
        edge_index: &PlanEdgeIndex,
        planner_config: &PlannerConfig,
        max_steps: usize,
        dt: f64,
        device: &TrainDevice,
    ) -> Option<f64> {
        let mut state = GraphState::new(units, &[UnitKind::Commander]);
        let mut shortfall = [0.0f32; 3];

        for _ in 0..max_steps {
            if state.goal_reached(goal) {
                return Some(state.time);
            }

            let direction_mask = edge_index.legal_category_mask(&state, units, planner_config);
            if direction_mask.iter().all(|&b| !b) {
                state.tick(units, dt);
                continue;
            }

            let macro_features =
                state_features_with_shortfall(&state, units, planner_config, shortfall);
            let direction_logits = model.evaluate_direction(macro_features.clone(), device);
            let direction_idx = masked_argmax(&direction_logits, &direction_mask).unwrap_or(0);
            let category = EdgeCategory::ALL[direction_idx];

            let action_mask =
                edge_index.legal_mask_for_category(&state, units, planner_config, category);
            if action_mask.iter().all(|&b| !b) {
                state.tick(units, dt);
                continue;
            }

            let action_logits = model.evaluate_action(macro_features.clone(), category, device);
            let edge_idx = masked_argmax(&action_logits, &action_mask).unwrap_or(0);

            let edge = match edge_index.get(edge_idx) {
                Some(e) => e.clone(),
                None => {
                    state.tick(units, dt);
                    continue;
                }
            };

            let power_mean =
                model.evaluate_power(macro_features.clone(), edge_idx, edge_index.len(), device);
            let target_power = power_mean.max(0.0).round();

            let squad_raw = model.evaluate_squad(macro_features, target_power, device);
            let squad_raw_arr = [
                squad_raw.get(0).copied().unwrap_or(0.0),
                squad_raw.get(1).copied().unwrap_or(0.0),
                squad_raw.get(2).copied().unwrap_or(0.0),
            ];

            let available = idle_engineer_counts(&state, units);
            let mut desired = clamp_squad(squad_raw_arr, available);
            desired = ensure_minimum_squad(desired, available);

            let builders = select_squad_for_edge(&edge, desired, &state, units);
            if builders.is_empty() {
                shortfall = shortfall_from_counts(desired, available);
                state.tick(units, dt);
                continue;
            }

            let action = match edge.kind {
                crate::planner::plan_graph::PlanEdgeKind::Build => SimAction::Build {
                    unit_id: edge.target.clone(),
                    builders: builders.clone(),
                },
                crate::planner::plan_graph::PlanEdgeKind::Upgrade => SimAction::Upgrade {
                    target_unit_id: edge.target.clone(),
                    old_node: find_upgrade_source(&state, &edge.source)
                        .unwrap_or_else(|| crate::sim::NodeId::new(0)),
                    builders: builders.clone(),
                },
            };

            if execute_action(&mut state, &action, units, dt).is_err() {
                shortfall = shortfall_from_counts(desired, available);
                state.tick(units, dt);
                continue;
            }

            let assigned_counts = assigned_squad_counts(&state, &builders);
            shortfall = shortfall_from_counts(desired, assigned_counts);
        }

        None
    }

    fn compute_returns(&mut self, episode: &mut Episode) {
        let step_count = episode.steps.len();
        if step_count == 0 {
            return;
        }

        // Discounted returns from each step, including the terminal bonus at the
        // end of the episode.
        let gamma = self.config.gamma;
        let mut returns = Vec::with_capacity(step_count);
        let mut g = episode.final_reward;
        for step in episode.steps.iter().rev() {
            g = step.step_reward + gamma * g;
            returns.push(g);
        }
        returns.reverse();

        let mean = returns.iter().sum::<f32>() / step_count as f32;
        let std = (returns.iter().map(|r| (r - mean).powi(2)).sum::<f32>() / step_count as f32)
            .sqrt()
            .max(1e-6);

        for (step, ret) in episode.steps.iter_mut().zip(returns) {
            step.return_value = (ret - mean) / std;
        }
    }

    /// Update the hierarchical policy network from one episode using REINFORCE.
    pub(crate) fn update(&mut self, episode: &Episode) -> f32 {
        let mut accumulated_loss: Option<Tensor<TrainBackend, 1>> = None;
        let mut total_loss = 0.0f32;
        let mut step_count = 0usize;

        for step in &episode.steps {
            let base_features = step.base_features.clone();
            let num_edges = step.action_mask.len();

            let macro_features = {
                let mut v = base_features.clone();
                v.extend_from_slice(&step.shortfall);
                v
            };
            let macro_input = tensor1d_from_vec(&macro_features);
            let latent = self.model.latent(macro_input);

            // Direction head.
            let direction_logits = self
                .model
                .direction_logits(latent.clone())
                .flatten::<1>(0, 1);
            let direction_mask: Vec<f32> = step
                .direction_mask
                .iter()
                .map(|&legal| if legal { 0.0 } else { MASK_VALUE })
                .collect();
            let direction_mask_tensor = Tensor::<TrainBackend, 1>::from_data(
                TensorData::new(direction_mask, [DIRECTION_COUNT]),
                &self.device,
            );
            let masked_direction_logits = direction_logits + direction_mask_tensor;
            let direction_log_probs = log_softmax(masked_direction_logits, 0);
            let direction_index_tensor = Tensor::<TrainBackend, 1, burn::tensor::Int>::from_data(
                TensorData::new(vec![step.direction_index as i64], [1]),
                &self.device,
            );
            let direction_log_prob = direction_log_probs
                .clone()
                .select(0, direction_index_tensor);

            // Action head (conditioned on the chosen direction).
            let direction_one_hot = Tensor::<TrainBackend, 2>::from_data(
                TensorData::new(
                    one_hot(step.direction_index, DIRECTION_COUNT),
                    [1, DIRECTION_COUNT],
                ),
                &self.device,
            );
            let action_logits = self
                .model
                .action_logits(latent.clone(), direction_one_hot)
                .flatten::<1>(0, 1);
            let action_mask: Vec<f32> = step
                .action_mask
                .iter()
                .map(|&legal| if legal { 0.0 } else { MASK_VALUE })
                .collect();
            let action_mask_tensor = Tensor::<TrainBackend, 1>::from_data(
                TensorData::new(action_mask, [num_edges]),
                &self.device,
            );
            let masked_action_logits = action_logits + action_mask_tensor;
            let action_log_probs = log_softmax(masked_action_logits, 0);
            let edge_index_tensor = Tensor::<TrainBackend, 1, burn::tensor::Int>::from_data(
                TensorData::new(vec![step.edge_index as i64], [1]),
                &self.device,
            );
            let action_log_prob = action_log_probs.clone().select(0, edge_index_tensor);

            // Entropy bonus over both the direction and action distributions.
            let direction_probs = direction_log_probs.clone().exp();
            let direction_entropy = (direction_probs * direction_log_probs).neg().sum();
            let action_probs = action_log_probs.clone().exp();
            let action_entropy = (action_probs * action_log_probs).neg().sum();
            let entropy = direction_entropy + action_entropy;

            // Build-power network.
            let edge_one_hot = Tensor::<TrainBackend, 2>::from_data(
                TensorData::new(one_hot(step.edge_index, num_edges), [1, num_edges]),
                &self.device,
            );
            let power_mean = self
                .model
                .power_mean(latent.clone(), edge_one_hot)
                .flatten::<1>(0, 1);
            let power_log_prob = gaussian_log_prob_scalar(
                power_mean,
                step.target_power,
                self.config.power_std,
                &self.device,
            );

            // Engineer-squad network.
            let power_tensor = Tensor::<TrainBackend, 2>::from_data(
                TensorData::new(vec![step.target_power], [1, 1]),
                &self.device,
            );
            let squad_means = self
                .model
                .squad_means(latent, power_tensor)
                .flatten::<1>(0, 1);
            let squad_log_prob = gaussian_log_prob_vec(
                squad_means,
                &step.desired_squad,
                self.config.squad_std,
                &self.device,
            );

            let joint_log_prob =
                direction_log_prob + action_log_prob + power_log_prob + squad_log_prob;
            let return_tensor = Tensor::<TrainBackend, 1>::from_data(
                TensorData::new(vec![step.return_value], [1]),
                &self.device,
            );
            let policy_loss = joint_log_prob.neg().mul(return_tensor);
            let entropy_loss = entropy.neg().mul_scalar(self.config.entropy_coef);
            let loss = policy_loss + entropy_loss;

            total_loss += loss.clone().into_data().as_slice::<f32>().unwrap()[0];
            accumulated_loss = Some(match accumulated_loss {
                Some(acc) => acc + loss,
                None => loss,
            });
            step_count += 1;
        }

        if let Some(loss) = accumulated_loss {
            let grads = loss.backward();
            let grads = burn::optim::GradientsParams::from_grads(grads, &self.model);
            self.model =
                self.optimizer
                    .step(self.config.learning_rate.into(), self.model.clone(), grads);
        }

        if step_count == 0 {
            0.0
        } else {
            total_loss / step_count as f32
        }
    }

    /// Fine-tune the best model on a recorded trajectory using supervised loss.
    pub(crate) fn fine_tune_on_trajectory(
        &mut self,
        trajectory: &BuildTrajectory,
        units: &Units,
        goal: &UnitKind,
        planner_config: &PlannerConfig,
    ) -> f32 {
        if trajectory.steps.is_empty() {
            return 0.0;
        }

        let plan = units
            .plan_graph(goal)
            .expect("goal must be reachable for fine-tuning");
        let edge_index = PlanEdgeIndex::new(&plan);
        let mut state = GraphState::new(units, &[UnitKind::Commander]);
        let mut accumulated_loss: Option<Tensor<TrainBackend, 1>> = None;
        let mut total_loss_value = 0.0f32;
        let mut step_count = 0usize;

        for step in &trajectory.steps {
            let mut executable = false;
            for _ in 0..self.config.max_steps {
                let current_mask = edge_index.legal_mask(&state, units, planner_config);
                if step.edge_index >= current_mask.len() || !current_mask[step.edge_index] {
                    state.tick(units, planner_config.dt);
                    continue;
                }

                let base_features = state_features(&state, units, planner_config);
                let num_edges = edge_index.len();

                let category = edge_index
                    .get(step.edge_index)
                    .map(|e| e.category())
                    .unwrap_or(EdgeCategory::Progress);
                let direction_index = category as usize;

                let macro_features: Vec<f32> = {
                    let mut v = base_features.clone();
                    v.extend_from_slice(&step.shortfall);
                    v
                };
                let macro_input = tensor1d_from_vec(&macro_features);
                let latent = self.model.latent(macro_input);

                // Direction cross-entropy loss.
                let direction_logits = self
                    .model
                    .direction_logits(latent.clone())
                    .flatten::<1>(0, 1);
                let direction_mask = edge_index.legal_category_mask(&state, units, planner_config);
                let direction_mask_tensor = Tensor::<TrainBackend, 1>::from_data(
                    TensorData::new(
                        direction_mask
                            .iter()
                            .map(|&legal| if legal { 0.0 } else { MASK_VALUE })
                            .collect(),
                        [DIRECTION_COUNT],
                    ),
                    &self.device,
                );
                let masked_direction_logits = direction_logits + direction_mask_tensor;
                let direction_log_probs = log_softmax(masked_direction_logits, 0);
                let direction_index_tensor =
                    Tensor::<TrainBackend, 1, burn::tensor::Int>::from_data(
                        TensorData::new(vec![direction_index as i64], [1]),
                        &self.device,
                    );
                let direction_ce = direction_log_probs.select(0, direction_index_tensor).neg();

                // Action cross-entropy loss (conditioned on the recorded direction).
                let direction_one_hot = Tensor::<TrainBackend, 2>::from_data(
                    TensorData::new(
                        one_hot(direction_index, DIRECTION_COUNT),
                        [1, DIRECTION_COUNT],
                    ),
                    &self.device,
                );
                let action_logits = self
                    .model
                    .action_logits(latent.clone(), direction_one_hot)
                    .flatten::<1>(0, 1);
                let action_mask_tensor = Tensor::<TrainBackend, 1>::from_data(
                    TensorData::new(
                        current_mask
                            .iter()
                            .map(|&legal| if legal { 0.0 } else { MASK_VALUE })
                            .collect(),
                        [num_edges],
                    ),
                    &self.device,
                );
                let masked_action_logits = action_logits + action_mask_tensor;
                let action_log_probs = log_softmax(masked_action_logits, 0);
                let edge_index_tensor = Tensor::<TrainBackend, 1, burn::tensor::Int>::from_data(
                    TensorData::new(vec![step.edge_index as i64], [1]),
                    &self.device,
                );
                let action_ce = action_log_probs.select(0, edge_index_tensor).neg();

                // Build-power MSE loss.
                let edge_one_hot = Tensor::<TrainBackend, 2>::from_data(
                    TensorData::new(one_hot(step.edge_index, num_edges), [1, num_edges]),
                    &self.device,
                );
                let power_mean = self
                    .model
                    .power_mean(latent.clone(), edge_one_hot)
                    .flatten::<1>(0, 1);
                let target_power = Tensor::<TrainBackend, 1>::from_data(
                    TensorData::new(vec![step.target_power], [1]),
                    &self.device,
                );
                let power_diff = power_mean - target_power;
                let power_mse = (power_diff.clone() * power_diff).sum();

                // Engineer-squad MSE loss.
                let power_tensor = Tensor::<TrainBackend, 2>::from_data(
                    TensorData::new(vec![step.target_power], [1, 1]),
                    &self.device,
                );
                let squad_means = self
                    .model
                    .squad_means(latent, power_tensor)
                    .flatten::<1>(0, 1);
                let target_squad = Tensor::<TrainBackend, 1>::from_data(
                    TensorData::new(step.desired_squad.to_vec(), [3]),
                    &self.device,
                );
                let squad_diff = squad_means - target_squad;
                let squad_mse = (squad_diff.clone() * squad_diff).sum();

                let step_loss = direction_ce + action_ce + power_mse + squad_mse;
                total_loss_value += step_loss.clone().into_data().as_slice::<f32>().unwrap()[0];
                accumulated_loss = Some(match accumulated_loss {
                    Some(acc) => acc + step_loss,
                    None => step_loss,
                });
                step_count += 1;

                let option = edge_index
                    .to_selection_option(step.edge_index, &state, units, planner_config)
                    .expect("legal edge must produce a selection option");
                let available = idle_engineer_counts(&state, units);
                let mut desired = clamp_squad(step.desired_squad, available);
                desired = ensure_minimum_squad(desired, available);
                let edge = edge_index
                    .get(step.edge_index)
                    .expect("valid edge index")
                    .clone();
                let builders = select_squad_for_edge(&edge, desired, &state, units);

                let action = match option {
                    crate::planner::mcts::selections::SelectionOption::Build(target) => {
                        SimAction::Build {
                            unit_id: target,
                            builders,
                        }
                    }
                    crate::planner::mcts::selections::SelectionOption::Upgrade { from, to } => {
                        SimAction::Upgrade {
                            target_unit_id: to,
                            old_node: find_upgrade_source(&state, &from)
                                .unwrap_or_else(|| crate::sim::NodeId::new(0)),
                            builders,
                        }
                    }
                    crate::planner::mcts::selections::SelectionOption::Assist(_) => {
                        unreachable!("assist is not a plan-graph edge")
                    }
                };

                let _ = execute_action(&mut state, &action, units, planner_config.dt);
                executable = true;
                break;
            }

            if !executable {
                break;
            }
        }

        if let Some(loss) = accumulated_loss {
            let grads = loss.backward();
            let grads = burn::optim::GradientsParams::from_grads(grads, &self.model);
            self.model =
                self.optimizer
                    .step(self.config.learning_rate.into(), self.model.clone(), grads);
        }

        if step_count == 0 {
            0.0
        } else {
            total_loss_value / step_count as f32
        }
    }

    /// Consume the trainer and return the trained policy bundle.
    pub fn into_model(self) -> PolicyBundle<TrainBackend> {
        self.model
    }
}
