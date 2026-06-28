//! Policy-gradient training for the MLP value network.
//!
//! Uses REINFORCE: roll out episodes with the current policy, then update the
//! network so that actions leading to higher rewards become more likely.

use std::f32;

use burn::backend::{Autodiff, NdArray};
use burn::module::Module;
use burn::optim::adaptor::OptimizerAdaptor;
use burn::optim::{Adam, AdamConfig, Optimizer};
use burn::record::{CompactRecorder, Recorder};
use burn::tensor::activation::{log_softmax, softmax};
use burn::tensor::backend::AutodiffBackend;
use burn::tensor::{Tensor, TensorData};
use rand::distributions::WeightedIndex;
use rand::prelude::*;

use super::execute_action;
use super::features::{candidate_features, state_features, FEATURE_COUNT};
use super::selections::SelectionPools;
use super::value_net::ValueNet;
use crate::planner::core::PlannerConfig;
use crate::planner::plan_graph::PlanGraph;
use crate::sim::GraphState;
use crate::units::{UnitKind, Units};

/// Autodiff backend used for training.
pub type TrainBackend = Autodiff<NdArray>;

/// Device used for training.
pub type TrainDevice = burn::tensor::Device<TrainBackend>;

/// Configuration for a training run.
#[derive(Debug, Clone, Copy)]
pub struct TrainConfig {
    /// Number of episodes to run.
    pub episodes: usize,
    /// Maximum simulator steps per episode.
    pub max_steps: usize,
    /// Fixed simulator timestep for rollouts.
    pub dt: f64,
    /// Learning rate for Adam.
    pub learning_rate: f64,
    /// Discount factor for future rewards.
    pub gamma: f32,
    /// Probability of taking a random action during training (epsilon-greedy
    /// exploration on top of the softmax policy).
    pub epsilon: f32,
    /// Entropy bonus coefficient. Higher values encourage more exploration by
    /// keeping the policy distribution spread out.
    pub entropy_coef: f32,
}

impl Default for TrainConfig {
    fn default() -> Self {
        Self {
            episodes: 50,
            max_steps: 500,
            dt: 10.0,
            learning_rate: 1e-3,
            gamma: 0.99,
            epsilon: 0.1,
            entropy_coef: 0.01,
        }
    }
}

/// Statistics returned after a training run.
#[derive(Debug, Default, Clone)]
pub struct TrainStats {
    /// Number of episodes that reached the goal.
    pub goal_reaches: usize,
    /// Completion time for each successful episode.
    pub completion_times: Vec<f64>,
    /// Number of steps in each episode.
    pub episode_lengths: Vec<usize>,
    /// Average loss per episode.
    pub losses: Vec<f32>,
}

/// One recorded step in a training episode.
#[derive(Debug, Clone)]
struct EpisodeStep {
    /// Feature vector for each candidate at this step.
    candidate_features: Vec<Vec<f32>>,
    /// Index of the candidate that was selected.
    action_index: usize,
    /// Discounted return from this step onward.
    return_value: f32,
}

/// One complete training episode.
#[derive(Debug, Default, Clone)]
struct Episode {
    steps: Vec<EpisodeStep>,
    reached_goal: bool,
    completion_time: f64,
    /// Shaped reward computed from the final state.
    final_reward: f32,
}

/// Trainer for the MLP policy network.
/// Concrete optimizer type returned by `AdamConfig::init`.
type AdamOptimizer = OptimizerAdaptor<Adam, ValueNet<TrainBackend>, TrainBackend>;

pub struct Trainer {
    model: ValueNet<TrainBackend>,
    optimizer: AdamOptimizer,
    config: TrainConfig,
    device: TrainDevice,
    rng: ThreadRng,
    /// Running mean of episode returns for baseline subtraction.
    return_mean: f32,
    /// Running variance of episode returns for normalization.
    return_var: f32,
    /// Number of episodes used to estimate the return statistics.
    return_count: f32,
}

impl Trainer {
    /// Create a new trainer with random initialization.
    pub fn new(config: TrainConfig) -> Self {
        let device: TrainDevice = Default::default();
        let model = ValueNet::new(&device);
        let optimizer = AdamConfig::new().init();
        Self {
            model,
            optimizer,
            config,
            device,
            rng: thread_rng(),
            return_mean: 0.0,
            return_var: 0.0,
            return_count: 0.0,
        }
    }

    /// Train the policy on the given goal.
    pub fn train(&mut self, units: &Units, goal: &UnitKind) -> TrainStats {
        let plan = units
            .plan_graph(goal)
            .expect("goal must be reachable for training");
        let planner_config = PlannerConfig::default();
        let mut stats = TrainStats::default();

        for _ in 0..self.config.episodes {
            let episode = self.run_episode(units, goal, &plan, &planner_config);

            if !episode.steps.is_empty() {
                let loss = self.update(&episode);
                stats.losses.push(loss);
            }

            stats.episode_lengths.push(episode.steps.len());
            if episode.reached_goal {
                stats.goal_reaches += 1;
                stats.completion_times.push(episode.completion_time);
            }
        }

        stats
    }

    /// Run one episode and record the trajectory.
    fn run_episode(
        &mut self,
        units: &Units,
        goal: &UnitKind,
        plan: &PlanGraph,
        planner_config: &PlannerConfig,
    ) -> Episode {
        let mut state = GraphState::new(units, &[UnitKind::Commander]);
        let mut episode = Episode {
            reached_goal: false,
            completion_time: 0.0,
            final_reward: 0.0,
            steps: Vec::new(),
        };

        for _ in 0..self.config.max_steps {
            if state.goal_reached(goal) {
                episode.reached_goal = true;
                episode.completion_time = state.time;
                break;
            }

            let pools = SelectionPools::new(plan, &state, units);
            let candidates = pools.options().to_vec();

            if candidates.is_empty() {
                // No legal action; let time advance.
                state.tick(units, self.config.dt);
                continue;
            }

            let state_feats = state_features(&state, goal, units, planner_config);
            let candidate_features: Vec<Vec<f32>> = candidates
                .iter()
                .map(|c| {
                    let mut f = state_feats.clone();
                    f.extend(candidate_features(c, &state, plan, units));
                    f
                })
                .collect();

            let action_index = if self.rng.gen::<f32>() < self.config.epsilon {
                self.rng.gen_range(0..candidates.len())
            } else {
                sample_action_index(
                    &self.model,
                    &candidate_features,
                    &self.device,
                    &mut self.rng,
                )
            };

            let selected = &candidates[action_index];
            let Some(action) = selected.to_sim_action(&state, units) else {
                // Chosen candidate is no longer executable; wait a tick.
                state.tick(units, self.config.dt);
                continue;
            };

            if execute_action(&mut state, &action, units, self.config.dt).is_err() {
                // Illegal action (e.g., builder became busy); wait a tick.
                state.tick(units, self.config.dt);
                continue;
            }

            episode.steps.push(EpisodeStep {
                candidate_features,
                action_index,
                return_value: 0.0, // filled in after the episode ends
            });
        }

        episode.final_reward = compute_progress_reward(&state, units, goal, plan);
        self.compute_returns(&mut episode);
        episode
    }

    /// Fill in the normalized return for each step from the final reward.
    ///
    /// Because the reward is computed from the final state only, every step in
    /// the episode receives the same raw return. A running mean/variance
    /// baseline across episodes is used for normalization instead of a per-
    /// episode mean (which would collapse to zero).
    fn compute_returns(&mut self, episode: &mut Episode) {
        let step_count = episode.steps.len();
        if step_count == 0 {
            return;
        }

        let raw_return = episode.final_reward;

        // Update running mean and variance using Welford's algorithm.
        self.return_count += 1.0;
        let delta = raw_return - self.return_mean;
        self.return_mean += delta / self.return_count;
        let delta2 = raw_return - self.return_mean;
        self.return_var += delta * delta2;

        let std = (self.return_var / self.return_count).sqrt().max(1e-6);
        let normalized = (raw_return - self.return_mean) / std;

        for step in &mut episode.steps {
            step.return_value = normalized;
        }
    }

    /// Update the network from one episode using REINFORCE.
    fn update(&mut self, episode: &Episode) -> f32 {
        let mut total_loss = 0.0f32;
        let mut update_count = 0usize;

        for step in &episode.steps {
            let n = step.candidate_features.len();
            if n == 0 {
                continue;
            }

            let data = TensorData::new(
                step.candidate_features.iter().flatten().cloned().collect(),
                [n, FEATURE_COUNT],
            );
            let input = Tensor::<TrainBackend, 2>::from_data(data, &self.device);
            let scores = self.model.forward(input).flatten::<1>(0, 1);
            let log_probs = log_softmax(scores, 0);
            let index_tensor = Tensor::<TrainBackend, 1, burn::tensor::Int>::from_data(
                TensorData::new(vec![step.action_index as i64], [1]),
                &self.device,
            );
            let selected_log_prob = log_probs.clone().select(0, index_tensor);

            let return_tensor = Tensor::<TrainBackend, 1>::from_data(
                TensorData::new(vec![step.return_value], [1]),
                &self.device,
            );
            let policy_loss = selected_log_prob.neg().mul(return_tensor);

            // Entropy bonus: encourage exploration by penalizing low entropy.
            let probs = log_probs.clone().exp();
            let entropy = (probs * log_probs).neg().sum();
            let entropy_loss = entropy.neg().mul_scalar(self.config.entropy_coef);

            let loss = policy_loss + entropy_loss;

            let grads = loss.backward();
            let grads = burn::optim::GradientsParams::from_grads(grads, &self.model);
            self.model =
                self.optimizer
                    .step(self.config.learning_rate.into(), self.model.clone(), grads);

            total_loss += loss.into_data().as_slice::<f32>().unwrap()[0];
            update_count += 1;
        }

        if update_count == 0 {
            0.0
        } else {
            total_loss / update_count as f32
        }
    }

    /// Consume the trainer and return the trained model.
    pub fn into_model(self) -> ValueNet<TrainBackend> {
        self.model
    }
}

/// Compute a shaped reward from the final state of an episode.
///
/// The reward encourages:
/// - owning units on the plan graph, weighted by proximity to the goal;
/// - reaching higher factory and engineer tech tiers;
/// - completing the goal quickly.
fn compute_progress_reward(
    state: &GraphState,
    units: &Units,
    goal: &UnitKind,
    plan: &PlanGraph,
) -> f32 {
    use crate::units::TechLevel;

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

/// Sample an action from the softmax over candidate scores.
fn sample_action_index<B: AutodiffBackend>(
    model: &ValueNet<B>,
    candidate_features: &[Vec<f32>],
    device: &burn::tensor::Device<B>,
    rng: &mut ThreadRng,
) -> usize {
    let n = candidate_features.len();
    let data = TensorData::new(
        candidate_features.iter().flatten().cloned().collect(),
        [n, FEATURE_COUNT],
    );
    let input = Tensor::<B, 2>::from_data(data, device);
    let scores = model.forward(input).flatten::<1>(0, 1);
    let probs = softmax(scores, 0);
    let prob_vec: Vec<f32> = probs.into_data().as_slice::<f32>().unwrap().to_vec();

    // WeightedIndex handles numerical normalization better than raw softmax.
    let dist = WeightedIndex::new(&prob_vec)
        .unwrap_or_else(|_| WeightedIndex::new(vec![1.0f32; n]).unwrap());
    dist.sample(rng)
}

/// Train a policy for `goal` and return the trained model.
pub fn train_policy(
    units: &Units,
    goal: &UnitKind,
    config: TrainConfig,
) -> (ValueNet<TrainBackend>, TrainStats) {
    let mut trainer = Trainer::new(config);
    let stats = trainer.train(units, goal);
    let model = trainer.into_model();
    (model, stats)
}

/// Save a trained model to disk.
pub fn save_model(model: &ValueNet<TrainBackend>, path: &std::path::Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("failed to create model dir: {e}"))?;
    }
    let recorder = CompactRecorder::new();
    recorder
        .record(model.clone().into_record(), path.to_path_buf())
        .map_err(|e| format!("failed to save model: {e}"))
}

/// Load a trained model from disk.
pub fn load_model(path: &std::path::Path) -> Result<ValueNet<TrainBackend>, String> {
    let device: TrainDevice = Default::default();
    let recorder = CompactRecorder::new();
    let record = recorder
        .load(path.to_path_buf(), &device)
        .map_err(|e| format!("failed to load model: {e}"))?;
    Ok(ValueNet::new(&device).load_record(record))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{UnitId, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn trainer_runs_episodes_without_panic() {
        let units = load_units();
        let goal = UnitKind::Unique(UnitId("UEL0401".to_string()));
        let mut trainer = Trainer::new(TrainConfig {
            episodes: 3,
            max_steps: 50,
            ..Default::default()
        });

        let stats = trainer.train(&units, &goal);

        assert_eq!(stats.episode_lengths.len(), 3);
    }

    #[test]
    fn save_and_load_model_round_trip() {
        let units = load_units();
        let goal = UnitKind::Unique(UnitId("UEL0401".to_string()));
        let mut trainer = Trainer::new(TrainConfig {
            episodes: 2,
            max_steps: 20,
            ..Default::default()
        });
        trainer.train(&units, &goal);
        let model = trainer.into_model();

        let dir = std::env::temp_dir().join("faf-sim-train-test");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("test-model");

        save_model(&model, &path).expect("save should succeed");
        let loaded = load_model(&path).expect("load should succeed");

        let device: TrainDevice = Default::default();
        let dummy = vec![0.0f32; FEATURE_COUNT];
        let before = model.evaluate_single(dummy.clone(), &device);
        let after = loaded.evaluate_single(dummy, &device);
        assert!(
            (before - after).abs() < 1e-3,
            "loaded model should produce approximately the same output as the saved model"
        );
    }
}
