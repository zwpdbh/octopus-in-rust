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
use burn::tensor::backend::AutodiffBackend;
use burn::tensor::activation::{log_softmax, softmax};
use burn::tensor::{Tensor, TensorData};
use rand::distributions::WeightedIndex;
use rand::prelude::*;

use super::features::{candidate_features, state_features, FEATURE_COUNT};
use super::pools::SelectionPools;
use super::value_net::ValueNet;
use super::{candidate_to_action, execute_action};
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
        let mut episode = Episode::default();

        for _ in 0..self.config.max_steps {
            if state.goal_reached(goal) {
                episode.reached_goal = true;
                episode.completion_time = state.time;
                break;
            }

            let pools = SelectionPools::derive(plan, &state, units);
            let candidates = pools.candidates();

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
                    f.extend(candidate_features(c, plan, units));
                    f
                })
                .collect();

            let action_index = if self.rng.gen::<f32>() < self.config.epsilon {
                self.rng.gen_range(0..candidates.len())
            } else {
                sample_action_index(&self.model, &candidate_features, &self.device, &mut self.rng)
            };

            let selected = &candidates[action_index];
            let Some(action) = candidate_to_action(selected, &state, units, plan) else {
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

        self.compute_returns(&mut episode);
        episode
    }

    /// Fill in the discounted return for each step from the final reward.
    fn compute_returns(&self, episode: &mut Episode) {
        let reward = if episode.reached_goal {
            // Higher reward for faster completion.
            (1.0 / (1.0 + episode.completion_time as f32)).clamp(1e-6, 1.0)
        } else {
            0.0
        };

        let step_count = episode.steps.len();
        for (t, step) in episode.steps.iter_mut().enumerate() {
            let steps_to_end = step_count - t;
            step.return_value = reward * self.config.gamma.powi(steps_to_end as i32);
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
            let selected_log_prob = log_probs.select(0, index_tensor);

            let return_tensor = Tensor::<TrainBackend, 1>::from_data(
                TensorData::new(vec![step.return_value], [1]),
                &self.device,
            );
            let loss = selected_log_prob.neg().mul(return_tensor);

            let grads = loss.backward();
            let grads = burn::optim::GradientsParams::from_grads(grads, &self.model);
            self.model = self
                .optimizer
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
    let dist = WeightedIndex::new(&prob_vec).unwrap_or_else(|_| WeightedIndex::new(vec![1.0f32; n]).unwrap());
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
}
