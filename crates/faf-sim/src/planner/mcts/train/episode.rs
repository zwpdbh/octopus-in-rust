//! Episode and trajectory data structures.

/// One recorded step in a training episode.
#[derive(Debug, Clone)]
pub(crate) struct EpisodeStep {
    /// Base state feature vector (without shortfall).
    pub(crate) base_features: Vec<f32>,
    /// Shortfall feedback fed into the macro network at this step.
    pub(crate) shortfall: [f32; 3],
    /// Legal-edge mask used to mask the macro logits.
    pub(crate) legal_mask: Vec<bool>,
    /// Index of the selected plan-graph edge.
    pub(crate) edge_index: usize,
    /// Target build power sampled for this edge.
    pub(crate) target_power: f32,
    /// Desired [T1, T2, T3] engineer counts sampled for this build power.
    pub(crate) desired_squad: [f32; 3],
    /// Normalized return for this step, filled in after the episode ends.
    pub(crate) return_value: f32,
}

/// One step of the best discovered trajectory, used for supervised fine-tuning.
#[derive(Debug, Clone)]
pub(crate) struct TrajectoryStep {
    pub(crate) edge_index: usize,
    pub(crate) target_power: f32,
    pub(crate) desired_squad: [f32; 3],
    pub(crate) shortfall: [f32; 3],
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
