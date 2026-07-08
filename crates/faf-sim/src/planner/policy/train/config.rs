//! Training configuration and statistics.

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
    /// Stop early when the best completion time is at most this many seconds.
    pub target_time: Option<f64>,

    /// Global gradient norm clipping threshold. `None` disables clipping.
    /// A value of `1.0` is a safe default for preventing REINFORCE divergence.
    pub grad_clip: Option<f32>,
    /// Maximum number of mass extractors (including capped upgrades) that may
    /// be active at the same time. New mex builds are blocked once this cap is
    /// reached; upgrades do not count toward the cap.
    pub max_mex_count: usize,
    /// Coefficient for the build-power delta reward.
    ///
    /// Each step is rewarded for increasing total active build power by
    /// `(next_bp - prev_bp) * reward_bp_coef`. Set to `0.0` to disable.
    pub reward_bp_coef: f32,
    /// Coefficient for the mass-income delta reward.
    ///
    /// Each step is rewarded for increasing net mass income by
    /// `(next_mass - prev_mass) * reward_mass_income_coef`. Set to `0.0` to
    /// disable.
    pub reward_mass_income_coef: f32,
    /// Coefficient for the energy-income delta reward.
    ///
    /// Set to `0.0` (the default) to let the agent learn power management from
    /// the energy stall penalty instead of a direct income bonus, which can
    /// encourage overbuilding power generators.
    pub reward_energy_income_coef: f32,
    /// Penalty applied each step when energy storage is empty (energy stall).
    pub energy_stall_penalty: f32,
    /// Penalty applied each step when mass storage is empty (mass stall).
    pub mass_stall_penalty: f32,

    // ===== Rollout-based reward hyperparameters =====
    /// Horizon in seconds for the phantom-goal eco rollout.
    pub eco_rollout_horizon_secs: f32,
    /// Maximum seconds to simulate when evaluating a real goal rush.
    pub rush_rollout_cap_secs: f32,
    /// Fraction of total build power assigned to the phantom/rush goal project.
    pub rollout_bp_fraction: f32,
    /// Energy-stall duration must exceed this many seconds to trigger a penalty.
    pub energy_stall_threshold_secs: f32,
    /// Stored mass above this fraction of capacity counts as hoarding.
    pub mass_storage_hoarding_ratio: f32,
    /// Coefficient scaling the delta in mass spent during the eco rollout.
    pub mass_reward_coef: f32,
    /// Fixed penalty when the chosen eco direction does not increase mass spent.
    pub wasted_action_penalty: f32,
    /// Penalty when stored mass exceeds the hoarding ratio at rollout end.
    pub hoarding_penalty: f32,
    /// Penalty when energy stall lasts longer than the threshold.
    pub stall_penalty: f32,
    /// Base reward for finishing the real goal within the rush cap.
    pub goal_finish_base_reward: f32,
    /// Additional reward per second saved under the rush cap.
    pub goal_time_reward_coef: f32,
    /// Penalty for picking Goal when the goal cannot finish within the rush cap.
    pub goal_too_early_penalty: f32,
    /// Initial epsilon for Goal-only exploration.
    pub epsilon_start: f32,
    /// Final epsilon for Goal-only exploration.
    pub epsilon_end: f32,
    /// Number of episodes over which epsilon decays linearly.
    pub epsilon_decay_episodes: usize,
    /// Rush probability threshold above which Goal is chosen (outside exploration).
    pub rush_threshold: f32,
    /// Weight for the rush-head loss when combined with the eco-head loss.
    pub rush_loss_weight: f32,
}

impl Default for TrainConfig {
    fn default() -> Self {
        Self {
            episodes: 200,
            max_steps: 500,
            dt: 1.0,
            learning_rate: 1e-3,
            gamma: 0.99,
            target_time: None,
            grad_clip: None,
            max_mex_count: 12,
            reward_bp_coef: 1.0 / 20.0,
            reward_mass_income_coef: 1.0 / 10.0,
            reward_energy_income_coef: 0.0,
            energy_stall_penalty: 20.0,
            mass_stall_penalty: 1.0,

            // Rollout-based reward defaults (tuned by experimentation).
            eco_rollout_horizon_secs: 60.0,
            rush_rollout_cap_secs: 300.0,
            rollout_bp_fraction: 0.8,
            energy_stall_threshold_secs: 5.0,
            mass_storage_hoarding_ratio: 0.5,
            mass_reward_coef: 1.0 / 100.0,
            wasted_action_penalty: 0.5,
            hoarding_penalty: 1.0,
            stall_penalty: 5.0,
            goal_finish_base_reward: 100.0,
            goal_time_reward_coef: 0.5,
            goal_too_early_penalty: -10.0,
            epsilon_start: 0.3,
            epsilon_end: 0.01,
            epsilon_decay_episodes: 1000,
            rush_threshold: 0.5,
            rush_loss_weight: 1.0,
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
