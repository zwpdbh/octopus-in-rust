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
    /// Penalty applied when an episode hits the step limit without reaching the
    /// goal. A strong negative value makes timeouts clearly worse than any
    /// successful completion.
    pub timeout_penalty: f32,

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
            timeout_penalty: -1000.0,
            grad_clip: None,
            max_mex_count: 12,
            reward_bp_coef: 1.0 / 20.0,
            reward_mass_income_coef: 1.0 / 10.0,
            reward_energy_income_coef: 0.0,
            energy_stall_penalty: 20.0,
            mass_stall_penalty: 1.0,
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
