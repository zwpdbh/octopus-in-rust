//! Trainer for the standalone economy-expansion network.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use burn::optim::adaptor::OptimizerAdaptor;
use burn::optim::grad_clipping::GradientClippingConfig;
use burn::optim::{Adam, AdamConfig, Optimizer};
use burn::tensor::activation::log_softmax;
use burn::tensor::{Tensor, TensorData};
use rand::rngs::ThreadRng;

use super::config::TrainEcoConfig;
use super::eco_net::EcoNet;
use super::eco_reward::{compute_eco_step_reward, eco_episode_bonus};
use super::math::tensor1d_from_vec;
use super::{TrainBackend, TrainDevice};
use crate::engine::simulation_state::SimulationState;
use crate::planner::core::{Goal, PlannerConfig};
use crate::planner::plan_graph::{build_plan_graph, EdgeCategory};
use crate::planner::policy::direction_planner::execute_action;
use crate::planner::policy::features::state_features;
use crate::planner::policy::heuristic::{direction_to_action, is_direction_legal};
use crate::planner::policy::macro_net::{
    masked_argmax, masked_sample_index, ECO_DIRECTION_COUNT, ECO_DIRECTION_INDICES, MASK_VALUE,
};
use crate::units::{UnitKind, Units};

/// Optimizer type for the eco network.
pub type EcoAdamOptimizer = OptimizerAdaptor<Adam, EcoNet<TrainBackend>, TrainBackend>;

/// Statistics returned by the eco trainer.
#[derive(Debug, Default, Clone)]
pub struct EcoTrainStats {
    /// Number of episodes that reached the target mass income.
    pub target_hits: usize,
    /// Length of each episode in steps.
    pub episode_lengths: Vec<usize>,
    /// Average loss per episode.
    pub losses: Vec<f32>,
    /// Mass income reached at the end of each episode.
    pub final_mass_incomes: Vec<f64>,
}

/// Trainer for the standalone economy-expansion network.
pub struct EcoTrainer {
    model: EcoNet<TrainBackend>,
    optimizer: EcoAdamOptimizer,
    config: TrainEcoConfig,
    device: TrainDevice,
    rng: ThreadRng,
    stop_requested: Arc<AtomicBool>,
}

impl EcoTrainer {
    /// Create a new eco trainer with random initialization.
    pub fn new(config: TrainEcoConfig) -> Self {
        let device: TrainDevice = Default::default();
        let model = EcoNet::new(&device);
        Self::from_model(config, model)
    }

    /// Create an eco trainer from an existing model.
    pub fn from_model(config: TrainEcoConfig, model: EcoNet<TrainBackend>) -> Self {
        let device: TrainDevice = Default::default();
        let optimizer = {
            let adam = AdamConfig::new();
            let adam = if let Some(clip) = config.grad_clip {
                adam.with_grad_clipping(Some(GradientClippingConfig::Norm(clip)))
            } else {
                adam
            };
            adam.init()
        };
        Self {
            model,
            optimizer,
            config,
            device,
            rng: rand::rng(),
            stop_requested: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Request a graceful stop at the next episode boundary.
    pub fn request_stop(&self) {
        self.stop_requested.store(true, Ordering::Relaxed);
    }

    fn should_stop(&self) -> bool {
        self.stop_requested.load(Ordering::Relaxed)
    }

    /// Train the eco network and return the final model.
    /// During trainning, each ep's result is the time the model took
    /// to reach TrainEcoConfig's eco_target_mass_income.
    /// In such case, given the simulation state in which all build power are idle, the mass
    /// income should be equal or larger than eco_target_mass_income while energy is overflowing.
    pub fn train(&mut self, units: &Units) -> EcoTrainStats {
        let mut stats = EcoTrainStats::default();
        let planner_config = PlannerConfig {
            dt: self.config.dt,
            max_mex_count: self.config.max_mex_count,
            ..PlannerConfig::default()
        };
        let plan = build_plan_graph(units, Goal::default());

        for episode_idx in 0..self.config.episodes {
            if self.should_stop() {
                break;
            }

            let result = self.run_episode(units, &planner_config, &plan, episode_idx);

            let reached = result.final_mass_income >= self.config.eco_target_mass_income;
            if reached {
                stats.target_hits += 1;
            }
            stats.episode_lengths.push(result.episode.steps.len());
            stats.losses.push(result.avg_loss);
            stats.final_mass_incomes.push(result.final_mass_income);

            if episode_idx % 10 == 0 || reached {
                println!(
                    "ep={:4} steps={:4} reached={} mass_income={:8.1} loss={:9.4}",
                    episode_idx + 1,
                    result.episode.steps.len(),
                    reached,
                    result.final_mass_income,
                    result.avg_loss
                );
            }
        }

        stats
    }

    fn run_episode(
        &mut self,
        units: &Units,
        planner_config: &PlannerConfig,
        plan: &crate::planner::plan_graph::PlanGraph,
        episode_idx: usize,
    ) -> EcoEpisodeResult {
        let mut state = SimulationState::new(units, &[UnitKind::Commander]);
        let mut episode = EcoEpisode::default();
        let mut accumulated_loss = 0.0f32;
        let mut step_count = 0usize;

        for _step in 0..self.config.max_steps {
            if state.economy.net_mass_income.value() >= self.config.eco_target_mass_income {
                break;
            }

            let features = state_features(&state, units, planner_config);
            let direction_mask = legal_eco_direction_mask(&state, units, planner_config, plan);

            if direction_mask.iter().all(|&b| !b) {
                state.tick(units, self.config.dt);
                continue;
            }

            let eco_logits = self
                .model
                .evaluate_direction(features.clone(), &self.device);
            let epsilon = self.current_epsilon(episode_idx);
            let best_eco_idx = if self.should_explore(epsilon) {
                masked_sample_index(&eco_logits, &direction_mask, &mut self.rng)
            } else {
                masked_argmax(&eco_logits, &direction_mask)
            }
            .unwrap_or(0);

            let direction = EdgeCategory::ALL[ECO_DIRECTION_INDICES[best_eco_idx]];
            let action = direction_to_action(
                direction,
                &state,
                units,
                planner_config,
                &Goal::default(),
                plan,
            );

            let prev_state = state.clone();
            if execute_action(&mut state, &action, units, self.config.dt).is_err() {
                state.tick(units, self.config.dt);
                continue;
            }

            let mut reward = compute_eco_step_reward(&prev_state, &state, &self.config);
            if state.economy.net_mass_income.value() >= self.config.eco_target_mass_income {
                reward += eco_episode_bonus(&state, self.config.eco_target_mass_income);
            }

            let step = EcoEpisodeStep {
                features,
                direction_mask,
                direction_index: best_eco_idx,
                reward,
            };

            accumulated_loss += self.update_step(&step);
            step_count += 1;
            episode.steps.push(step);
        }

        let avg_loss = if step_count == 0 {
            0.0
        } else {
            accumulated_loss / step_count as f32
        };
        EcoEpisodeResult {
            episode,
            avg_loss,
            final_mass_income: state.economy.net_mass_income.value(),
        }
    }

    fn update_step(&mut self, step: &EcoEpisodeStep) -> f32 {
        let features = step.features.clone();
        let macro_input = tensor1d_from_vec(&features);
        let latent = self.model.latent(macro_input);
        let eco_logits = self.model.eco_logits(latent).flatten::<1>(0, 1);

        let eco_mask: Vec<f32> = step
            .direction_mask
            .iter()
            .map(|&legal| if legal { 0.0 } else { MASK_VALUE })
            .collect();
        let eco_mask_tensor = Tensor::<TrainBackend, 1>::from_data(
            TensorData::new(eco_mask, [ECO_DIRECTION_COUNT]),
            &self.device,
        );
        let masked_eco_logits = eco_logits + eco_mask_tensor;
        let eco_log_probs = log_softmax(masked_eco_logits, 0);

        let direction_index_tensor = Tensor::<TrainBackend, 1, burn::tensor::Int>::from_data(
            TensorData::new(vec![step.direction_index as i64], [1]),
            &self.device,
        );
        let eco_log_prob = eco_log_probs.select(0, direction_index_tensor);
        let reward_tensor = Tensor::<TrainBackend, 1>::from_data(
            TensorData::new(vec![step.reward], [1]),
            &self.device,
        );
        let loss = eco_log_prob.neg().mul(reward_tensor);
        let loss_value = loss.clone().into_data().as_slice::<f32>().unwrap()[0];

        let grads = loss.backward();
        let grads = burn::optim::GradientsParams::from_grads(grads, &self.model);
        self.model = self
            .optimizer
            .step(self.config.learning_rate, self.model.clone(), grads);

        loss_value
    }

    fn current_epsilon(&self, episode_idx: usize) -> f32 {
        let start = self.config.epsilon_start;
        let end = self.config.epsilon_end;
        let decay_episodes = self.config.epsilon_decay_episodes.max(1);
        let progress = (episode_idx as f32 / decay_episodes as f32).min(1.0);
        start + (end - start) * progress
    }

    fn should_explore(&self, epsilon: f32) -> bool {
        rand::random::<f32>() < epsilon
    }

    /// Consume the trainer and return the trained model.
    pub fn into_model(self) -> EcoNet<TrainBackend> {
        self.model
    }
}

#[derive(Debug, Default, Clone)]
struct EcoEpisode {
    steps: Vec<EcoEpisodeStep>,
}

/// Result of running a single eco-training episode.
#[derive(Debug, Clone)]
struct EcoEpisodeResult {
    /// Recorded steps of the episode.
    episode: EcoEpisode,
    /// Average per-step loss for the episode.
    avg_loss: f32,
    /// Net mass income reached at the end of the episode.
    final_mass_income: f64,
}

#[derive(Debug, Clone)]
struct EcoEpisodeStep {
    features: Vec<f32>,
    direction_mask: Vec<bool>,
    direction_index: usize,
    reward: f32,
}

/// Build a boolean mask over the five eco directions.
fn legal_eco_direction_mask(
    state: &SimulationState,
    units: &Units,
    config: &PlannerConfig,
    plan: &crate::planner::plan_graph::PlanGraph,
) -> Vec<bool> {
    ECO_DIRECTION_INDICES
        .iter()
        .map(|&i| {
            let direction = EdgeCategory::ALL[i];
            is_direction_legal(direction, state, units, config, &Goal::default(), plan)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_units() -> Units {
        let json = include_str!("../../../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn eco_trainer_runs_episodes_without_panic() {
        let units = load_units();
        let config = TrainEcoConfig {
            episodes: 2,
            max_steps: 5,
            ..Default::default()
        };
        let mut trainer = EcoTrainer::new(config);
        let stats = trainer.train(&units);
        assert_eq!(stats.episode_lengths.len(), 2);
    }
}
