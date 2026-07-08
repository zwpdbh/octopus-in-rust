//! Episode and trajectory data structures.

/// One recorded step in a training episode.
#[derive(Debug, Clone)]
pub(crate) struct EpisodeStep {
    /// State feature vector fed to the network at this step.
    pub(crate) base_features: Vec<f32>,
    /// Mask over [`EdgeCategory::ALL`] indicating which directions are legal.
    pub(crate) direction_mask: Vec<bool>,
    /// Selected strategic direction index (into [`EdgeCategory::ALL`]).
    pub(crate) direction_index: usize,
    /// Rush probability predicted by the rush head at this step.
    /// Kept for diagnostics even when not used in the current update.
    #[allow(dead_code)]
    pub(crate) rush_p: f32,
    /// Target for the rush head (1.0 = goal finishes within cap, 0.0 = not).
    pub(crate) rush_target: f32,
}

/// One complete training episode.
#[derive(Debug, Default, Clone)]
pub(crate) struct Episode {
    pub(crate) steps: Vec<EpisodeStep>,
    pub(crate) reached_goal: bool,
    pub(crate) completion_time: f64,
}
