//! Training events consumed by Burn-style metrics.

/// Summary of a finished training episode.
#[derive(Debug, Clone, Copy)]
pub struct EpisodeSummary {
    /// One-based episode index.
    pub episode: usize,
    /// Total number of episodes requested (0 if the run is unbounded).
    pub total_episodes: usize,
    /// Number of simulator steps taken in this episode.
    pub steps: usize,
    /// Epsilon-greedy exploration probability used for this episode.
    pub epsilon: f32,
    /// Whether the episode reached the goal.
    pub reached_goal: bool,
    /// Simulator time when the goal was reached, or the final simulator time.
    pub completion_time: f64,
    /// Average policy loss for this episode, if an update was performed.
    pub loss: Option<f32>,
}

/// Summary of a periodic greedy evaluation.
#[derive(Debug, Clone, Copy)]
pub struct GreedyEvalSummary {
    /// One-based episode index at which the evaluation was run.
    pub episode: usize,
    /// Best completion time observed so far across greedy evaluations.
    pub best_time: Option<f64>,
}

/// Fine-tuning progress report.
#[derive(Debug, Clone, Copy)]
pub struct FineTuneSummary {
    /// One-based epoch index.
    pub epoch: usize,
    /// Total number of fine-tuning epochs.
    pub total_epochs: usize,
    /// Loss on the best trajectory for this epoch.
    pub loss: f32,
}

/// A single training event that metrics can observe.
#[derive(Debug, Clone, Copy)]
pub enum TrainEvent {
    Episode(EpisodeSummary),
    GreedyEval(GreedyEvalSummary),
    FineTuneEpoch(FineTuneSummary),
}
