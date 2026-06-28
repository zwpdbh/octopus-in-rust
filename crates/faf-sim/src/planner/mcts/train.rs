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
    /// Initial probability of taking a random action during training
    /// (epsilon-greedy exploration on top of the softmax policy).
    pub epsilon: f32,
    /// Final epsilon value after decay. Only used when `epsilon_decay_episodes`
    /// is non-zero.
    pub epsilon_final: f32,
    /// Number of episodes over which to linearly decay `epsilon` to
    /// `epsilon_final`. `0` means no decay.
    pub epsilon_decay_episodes: usize,
    /// Entropy bonus coefficient. Higher values encourage more exploration by
    /// keeping the policy distribution spread out.
    pub entropy_coef: f32,
    /// Stop early when the best completion time is at most this many seconds.
    pub target_time: Option<f64>,
    /// Print per-episode progress to stderr.
    pub verbose: bool,
}

impl Default for TrainConfig {
    fn default() -> Self {
        Self {
            episodes: 200,
            max_steps: 500,
            dt: 1.0,
            learning_rate: 1e-3,
            gamma: 0.99,
            epsilon: 0.1,
            epsilon_final: 0.1,
            epsilon_decay_episodes: 0,
            entropy_coef: 0.01,
            target_time: None,
            verbose: false,
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
///
/// A step corresponds to a single decision tick in the simulator: the policy
/// scored all legal options, sampled one, and the simulator advanced by `dt`.
#[derive(Debug, Clone)]
struct EpisodeStep {
    /// Feature matrix with one row per legal option at this step.
    candidate_features: Vec<Vec<f32>>,
    /// Index of the option that was selected from the feature matrix.
    action_index: usize,
    /// Normalized return for this step, filled in after the episode ends.
    return_value: f32,
}

/// One complete training episode.
///
/// An episode is a single rollout: it starts from the ACU state, runs the
/// current policy for up to `max_steps` decision ticks, and ends when the goal
/// is reached or the step budget is exhausted. The recorded trajectory is used
/// for one REINFORCE policy-gradient update.
#[derive(Debug, Default, Clone)]
struct Episode {
    /// Sequence of decision steps recorded during the rollout.
    steps: Vec<EpisodeStep>,
    /// True if the goal unit was reached before `max_steps`.
    reached_goal: bool,
    /// Simulator time when the goal was reached, meaningful only if `reached_goal` is true.
    completion_time: f64,
    /// Shaped reward computed from the final state of the rollout.
    final_reward: f32,
}

/// Trainer for the MLP policy network.
/// Concrete optimizer type returned by `AdamConfig::init`.
type AdamOptimizer = OptimizerAdaptor<Adam, ValueNet<TrainBackend>, TrainBackend>;

pub struct Trainer {
    model: ValueNet<TrainBackend>,
    /// Best model seen so far, captured whenever a new best completion time is found.
    best_model: Option<ValueNet<TrainBackend>>,
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
        Self::from_model(config, model)
    }

    /// Create a trainer that continues from an existing model.
    pub fn from_model(config: TrainConfig, model: ValueNet<TrainBackend>) -> Self {
        let device: TrainDevice = Default::default();
        let optimizer = AdamConfig::new().init();
        Self {
            model,
            best_model: None,
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

        // When resuming, evaluate the loaded model greedily so the log's `best`
        // column reflects the baseline and we avoid saving a worse model.
        let mut best_time: Option<f64> = if self.best_model.is_some() {
            let baseline = self.evaluate_greedy(units, goal, &plan, &planner_config);
            if self.config.verbose {
                if let Some(t) = baseline {
                    eprintln!(
                        "Resumed model greedy baseline: {}",
                        format_time(t, true)
                    );
                } else {
                    eprintln!("Resumed model did not reach the goal in a greedy evaluation.");
                }
            }
            baseline
        } else {
            None
        };

        let mut ep = 0usize;
        loop {
            if self.config.episodes != 0 && ep >= self.config.episodes {
                break;
            }

            let epsilon = self.current_epsilon(ep);
            let episode = self.run_episode(units, goal, &plan, &planner_config, epsilon);

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
                    self.best_model = Some(self.model.clone());
                }
                if let Some(target) = self.config.target_time {
                    if episode.completion_time <= target {
                        target_hit = true;
                    }
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
    ///
    /// This is the core simulator/network loop:
    ///
    /// 1. Start from the ACU state.
    /// 2. Derive the legal selection options for the current state.
    /// 3. Score them with the current policy network and sample one.
    /// 4. Convert it into a concrete simulator command and execute it.
    /// 5. Record the feature matrix and chosen action for training.
    /// 6. Repeat until the goal is reached or the step budget runs out.
    /// 7. Compute the final reward and normalize returns across episodes.
    /// Compute the exploration probability for the current episode.
    ///
    /// If `epsilon_decay_episodes` is zero, epsilon stays at its initial value.
    /// Otherwise it linearly decays from `epsilon` to `epsilon_final` over the
    /// configured number of episodes.
    fn current_epsilon(&self, ep: usize) -> f32 {
        let decay = self.config.epsilon_decay_episodes;
        if decay == 0 || ep >= decay {
            return self.config.epsilon_final;
        }
        let progress = ep as f32 / decay as f32;
        self.config.epsilon - (self.config.epsilon - self.config.epsilon_final) * progress
    }

    /// Run a deterministic greedy evaluation of the current model.
    ///
    /// This is used when resuming to establish a baseline completion time for
    /// the loaded model. It always picks the highest-scoring candidate and does
    /// not record any training data.
    fn evaluate_greedy(
        &self,
        units: &Units,
        goal: &UnitKind,
        plan: &PlanGraph,
        planner_config: &PlannerConfig,
    ) -> Option<f64> {
        let mut state = GraphState::new(units, &[UnitKind::Commander]);

        for _ in 0..self.config.max_steps {
            if state.goal_reached(goal) {
                return Some(state.time);
            }

            let pools = SelectionPools::new(plan, &state, units, planner_config);
            let candidates = pools.options().to_vec();
            if candidates.is_empty() {
                state.tick(units, self.config.dt);
                continue;
            }

            let state_feats = state_features(&state, units, planner_config);
            let feature_matrix: Vec<Vec<f32>> = candidates
                .iter()
                .map(|c| {
                    let mut f = state_feats.clone();
                    f.extend(candidate_features(c, &state, plan, units));
                    f
                })
                .collect();

            let action_index = argmax_action_index(&self.model, &feature_matrix, &self.device);
            let selected = &candidates[action_index];
            let Some(action) = selected.to_sim_action(&state, units) else {
                state.tick(units, self.config.dt);
                continue;
            };

            if execute_action(&mut state, &action, units, self.config.dt).is_err() {
                state.tick(units, self.config.dt);
            }
        }

        None
    }

    fn run_episode(
        &mut self,
        units: &Units,
        goal: &UnitKind,
        plan: &PlanGraph,
        planner_config: &PlannerConfig,
        epsilon: f32,
    ) -> Episode {
        // Every episode begins with only the commander.
        let mut state = GraphState::new(units, &[UnitKind::Commander]);
        let mut episode = Episode {
            reached_goal: false,
            completion_time: 0.0,
            final_reward: 0.0,
            steps: Vec::new(),
        };

        // Main decision loop: one iteration per decision tick (dt seconds).
        for _ in 0..self.config.max_steps {
            // Stop early if the goal has already been achieved.
            if state.goal_reached(goal) {
                episode.reached_goal = true;
                episode.completion_time = state.time;
                break;
            }

            // 1. Derive the plan-graph-constrained, state-dependent action space.
            let pools = SelectionPools::new(plan, &state, units, planner_config);
            let candidates = pools.options().to_vec();

            if candidates.is_empty() {
                // No legal action available; let the simulator advance and try again.
                state.tick(units, self.config.dt);
                continue;
            }

            // 2. Build a feature matrix with one row per legal option.
            //    Each row is [state_features | candidate_features].
            let state_feats = state_features(&state, units, planner_config);
            let candidate_features: Vec<Vec<f32>> = candidates
                .iter()
                .map(|c| {
                    let mut f = state_feats.clone();
                    f.extend(candidate_features(c, &state, plan, units));
                    f
                })
                .collect();

            // 3. Sample an option: random exploration with probability epsilon,
            //    otherwise sample from the softmax over MLP scores.
            let action_index = if self.rng.gen::<f32>() < epsilon {
                self.rng.gen_range(0..candidates.len())
            } else {
                sample_action_index(
                    &self.model,
                    &candidate_features,
                    &self.device,
                    &mut self.rng,
                )
            };

            // 4. Convert the abstract option into a concrete simulator command.
            let selected = &candidates[action_index];
            let Some(action) = selected.to_sim_action(&state, units) else {
                // The option is no longer executable (e.g., builder became busy);
                // advance time and continue.
                state.tick(units, self.config.dt);
                continue;
            };

            // 5. Execute the command. If the simulator rejects it, just wait.
            if execute_action(&mut state, &action, units, self.config.dt).is_err() {
                state.tick(units, self.config.dt);
                continue;
            }

            // 6. Record the trajectory step. The return is filled in once the
            //    episode ends and the final reward is known.
            episode.steps.push(EpisodeStep {
                candidate_features,
                action_index,
                return_value: 0.0,
            });
        }

        // 7. Reward shaping from the final state, then normalize returns.
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
    ///
    /// Gradients are accumulated over every step in the episode and a single
    /// optimizer step is applied at the end. This is the standard REINFORCE
    /// pattern and is less noisy than updating after each individual step.
    fn update(&mut self, episode: &Episode) -> f32 {
        let mut accumulated_loss: Option<Tensor<TrainBackend, 1>> = None;
        let mut total_loss = 0.0f32;
        let mut step_count = 0usize;

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
            self.model = self
                .optimizer
                .step(self.config.learning_rate.into(), self.model.clone(), grads);
        }

        if step_count == 0 {
            0.0
        } else {
            total_loss / step_count as f32
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

/// Deterministically pick the candidate with the highest MLP score.
fn argmax_action_index<B: AutodiffBackend>(
    model: &ValueNet<B>,
    candidate_features: &[Vec<f32>],
    device: &burn::tensor::Device<B>,
) -> usize {
    let n = candidate_features.len();
    let data = TensorData::new(
        candidate_features.iter().flatten().cloned().collect(),
        [n, FEATURE_COUNT],
    );
    let input = Tensor::<B, 2>::from_data(data, device);
    let scores = model.forward(input).flatten::<1>(0, 1);
    let score_vec: Vec<f32> = scores.into_data().as_slice::<f32>().unwrap().to_vec();
    score_vec
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// Train a policy for `goal` and return the final and best-seen models.
///
/// The returned `best_model` is the model checkpoint captured when the fastest
/// completion time was observed during training. If no episode reached the
/// goal, it is `None`.
pub fn train_policy(
    units: &Units,
    goal: &UnitKind,
    config: TrainConfig,
) -> (ValueNet<TrainBackend>, Option<ValueNet<TrainBackend>>, TrainStats) {
    let mut trainer = Trainer::new(config);
    let stats = trainer.train(units, goal);
    let best_model = trainer.best_model.take();
    let model = trainer.into_model();
    (model, best_model, stats)
}

/// Continue training an existing policy for `goal`.
pub fn train_policy_from(
    model: ValueNet<TrainBackend>,
    units: &Units,
    goal: &UnitKind,
    config: TrainConfig,
) -> (ValueNet<TrainBackend>, Option<ValueNet<TrainBackend>>, TrainStats) {
    let mut trainer = Trainer::from_model(config, model);
    // Treat the loaded model as the initial best so training only replaces it
    // if it actually improves on the baseline.
    trainer.best_model = Some(trainer.model.clone());
    let stats = trainer.train(units, goal);
    let best_model = trainer.best_model.take();
    let model = trainer.into_model();
    (model, best_model, stats)
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

/// Format a duration in seconds as "Xm Y.Ys", or "-" if `valid` is false.
fn format_time(seconds: f64, valid: bool) -> String {
    if !valid {
        return "-".to_string();
    }
    let minutes = (seconds / 60.0).floor();
    let secs = seconds - minutes * 60.0;
    format!("{:.0}m {:.1}s", minutes, secs)
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
