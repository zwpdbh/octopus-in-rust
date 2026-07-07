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
}

/// One complete training episode.
#[derive(Debug, Default, Clone)]
pub(crate) struct Episode {
    pub(crate) steps: Vec<EpisodeStep>,
    pub(crate) reached_goal: bool,
    pub(crate) completion_time: f64,
}
