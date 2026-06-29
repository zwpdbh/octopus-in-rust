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
    /// Evaluate the current model greedily every N episodes and keep the best
    /// greedy model. `0` disables periodic greedy evaluation.
    pub greedy_eval_interval: usize,
    /// Number of supervised fine-tuning epochs to run on the best discovered
    /// trajectory after REINFORCE training.
    pub fine_tune_epochs: usize,
    /// Standard deviation for build-power sampling.
    pub power_std: f32,
    /// Standard deviation for engineer-count sampling.
    pub squad_std: f32,
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
            greedy_eval_interval: 100,
            fine_tune_epochs: 100,
            power_std: 2.0,
            squad_std: 0.5,
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
