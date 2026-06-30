//! Training progress observer.
//!
//! The `Trainer` can report coarse-grained training events to an implementor of
//! [`TrainingObserver`]. This lets external crates (for example a terminal
//! dashboard) display live progress without `faf-sim` depending on any UI
//! library.

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
    /// Best completion time observed so far (training or greedy).
    pub best_time: Option<f64>,
}

/// Summary of a periodic greedy evaluation.
#[derive(Debug, Clone, Copy)]
pub struct GreedyEvalSummary {
    /// One-based episode index at which the evaluation was run.
    pub episode: usize,
    /// Whether the greedy rollout reached the goal.
    pub reached_goal: bool,
    /// Completion time of the greedy rollout, if it reached the goal.
    pub completion_time: Option<f64>,
    /// Best completion time observed so far.
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

/// Trait for receiving progress updates from a [`Trainer`](super::Trainer).
///
/// All methods have default no-op implementations so observers only need to
/// implement the events they care about.
pub trait TrainingObserver: Send {
    /// Called when an episode finishes.
    fn on_episode_end(&mut self, _summary: EpisodeSummary) {}

    /// Called when a periodic greedy evaluation finishes.
    fn on_greedy_eval(&mut self, _summary: GreedyEvalSummary) {}

    /// Called after each supervised fine-tuning epoch on the best trajectory.
    fn on_fine_tune_epoch(&mut self, _summary: FineTuneSummary) {}

    /// Return `true` to request a graceful stop at the next episode boundary.
    fn should_stop(&self) -> bool {
        false
    }
}

impl TrainingObserver for () {
    fn should_stop(&self) -> bool {
        false
    }
}

impl<T: TrainingObserver + ?Sized> TrainingObserver for Box<T> {
    fn on_episode_end(&mut self, summary: EpisodeSummary) {
        (**self).on_episode_end(summary);
    }

    fn on_greedy_eval(&mut self, summary: GreedyEvalSummary) {
        (**self).on_greedy_eval(summary);
    }

    fn on_fine_tune_epoch(&mut self, summary: FineTuneSummary) {
        (**self).on_fine_tune_epoch(summary);
    }

    fn should_stop(&self) -> bool {
        (**self).should_stop()
    }
}
