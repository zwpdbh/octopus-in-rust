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
    /// Whether the episode reached the goal.
    pub reached_goal: bool,
    /// Simulator time when the goal was reached, or the final simulator time.
    pub completion_time: f64,
    /// Average policy loss for this episode, if an update was performed.
    pub loss: Option<f32>,
}

/// Training event types emitted by the policy trainer.
#[derive(Debug, Clone, Copy)]
pub enum TrainEvent {
    Episode(EpisodeSummary),
}
