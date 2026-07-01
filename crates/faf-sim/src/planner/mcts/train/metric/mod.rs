//! Burn-style training metrics and event types for faf-sim.

pub mod events;
pub mod metrics;

pub use events::{EpisodeSummary, FineTuneSummary, GreedyEvalSummary, TrainEvent};
pub use metrics::{
    training_progress, BestTimeMetric, CompletionTimeMetric, EpisodeLossMetric, EpisodeSpeedMetric,
    EpisodeStepsMetric, EpsilonMetric, FafSimMetrics, GoalReachMetric, GreedyEvalTimeMetric,
};
