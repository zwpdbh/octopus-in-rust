//! Episode and trajectory data structures.

/// One recorded step in a training episode.
#[derive(Debug, Clone)]
pub(crate) struct EpisodeStep {
    /// State feature vector fed to the direction head at this step.
    pub(crate) base_features: Vec<f32>,
    /// Mask over [`EdgeCategory::ALL`] indicating which directions are legal.
    pub(crate) direction_mask: Vec<bool>,
    /// Selected strategic direction index (into [`EdgeCategory::ALL`]).
    pub(crate) direction_index: usize,
    /// Raw reward received after this step.
    pub(crate) step_reward: f32,
    /// Normalized return for this step, filled in after the episode ends.
    pub(crate) return_value: f32,
}

/// One step of the best discovered trajectory, used for supervised fine-tuning.
#[derive(Debug, Clone)]
pub(crate) struct TrajectoryStep {
    pub(crate) direction_index: usize,
}

/// In-memory trajectory for the best training episode.
#[derive(Debug, Clone, Default)]
pub(crate) struct BuildTrajectory {
    pub(crate) steps: Vec<TrajectoryStep>,
}

/// One complete training episode.
#[derive(Debug, Default, Clone)]
pub(crate) struct Episode {
    pub(crate) steps: Vec<EpisodeStep>,
    pub(crate) reached_goal: bool,
    pub(crate) completion_time: f64,
    pub(crate) final_reward: f32,
}
