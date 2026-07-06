//! Burn-style metrics for faf-sim training.
//!
//! The training dashboard displays one plot per metric. Each metric is updated
//! from [`TrainEvent`]s produced by the MCTS planner. The following metrics are
//! collected:
//!
//! | Metric | Source | Interpretation |
//! |---|---|---|
//! | Episode Loss | `TrainEvent::Episode` | REINFORCE policy loss for the finished episode. Lower is better; a downward trend means the policy is improving. |
//! | Fine-Tune Loss | `TrainEvent::FineTuneEpoch` | Supervised fine-tuning loss on the best trajectories. Lower is better. |
//! | Episode Steps | `TrainEvent::Episode` | Number of simulator steps taken in the episode. Lower usually means the agent reached the goal faster. |
//! | Completion Time | `TrainEvent::Episode` | Simulator time when the episode ended (goal reached or step limit). Lower is better. |
//! | Goal Reach | `TrainEvent::Episode` | Percentage of episodes that reached the goal. Higher is better; 100% means the agent consistently succeeds. |
//! | Epsilon | `TrainEvent::Episode` | Current epsilon-greedy exploration probability. Starts high and decays; lower means less random exploration. |
//! | Best Time | `TrainEvent::Episode`, `TrainEvent::GreedyEval` | Best completion time observed so far across training and greedy evaluations. Lower is better. |
//! | Greedy Eval Time | `TrainEvent::GreedyEval` | Completion time of a periodic greedy (no exploration) evaluation. Lower is better. |
//! | Episodes/sec | `TrainEvent::Episode` | Training throughput, measured as episodes completed per wall-clock second. Higher is better. |

use std::sync::Arc;
use std::time::Instant;

use burn::data::dataloader::Progress;
use burn::train::metric::state::{FormatOptions, NumericMetricState};
use burn::train::metric::{
    Metric, MetricAttributes, MetricDefinition, MetricEntry, MetricId, MetricMetadata, MetricName,
    Numeric, NumericAttributes, NumericEntry, SerializedEntry,
};
use burn::train::renderer::{MetricState, MetricsRenderer, ProgressType, TrainingProgress};
use burn::train::LearnerSummary;

use super::events::{EpisodeSummary, FineTuneSummary, GreedyEvalSummary, TrainEvent};

/// Episode REINFORCE loss.
///
/// Tracks the policy loss computed over the completed episode. This is the
/// primary optimization objective: a decreasing value indicates the MCTS
/// policy network is learning from the collected trajectories.
#[derive(Clone, Default)]
pub struct EpisodeLossMetric {
    name: MetricName,
    state: NumericMetricState,
}

impl EpisodeLossMetric {
    pub fn new() -> Self {
        Self {
            name: Arc::new("Episode Loss".to_string()),
            state: NumericMetricState::default(),
        }
    }
}

impl Metric for EpisodeLossMetric {
    type Input = TrainEvent;

    fn name(&self) -> MetricName {
        self.name.clone()
    }

    fn attributes(&self) -> MetricAttributes {
        NumericAttributes {
            unit: None,
            higher_is_better: false,
        }
        .into()
    }

    fn update(&mut self, item: &Self::Input, _metadata: &MetricMetadata) -> SerializedEntry {
        let value = match item {
            TrainEvent::Episode(EpisodeSummary { loss: Some(l), .. }) => *l as f64,
            _ => return SerializedEntry::new("-".to_string(), "".to_string()),
        };
        self.state
            .update(value, 1, FormatOptions::new(self.name()).precision(4))
    }

    fn clear(&mut self) {
        self.state.reset();
    }
}

impl Numeric for EpisodeLossMetric {
    fn value(&self) -> NumericEntry {
        self.state.current_value()
    }

    fn running_value(&self) -> NumericEntry {
        self.state.running_value()
    }
}

/// Supervised fine-tuning loss.
///
/// Tracks the cross-entropy (or regression) loss when fine-tuning the policy
/// on the best trajectories collected so far. A decreasing value indicates the
/// policy is fitting the high-quality demonstrations.
#[derive(Clone, Default)]
pub struct FineTuneLossMetric {
    name: MetricName,
    state: NumericMetricState,
}

impl FineTuneLossMetric {
    pub fn new() -> Self {
        Self {
            name: Arc::new("Fine-Tune Loss".to_string()),
            state: NumericMetricState::default(),
        }
    }
}

impl Metric for FineTuneLossMetric {
    type Input = TrainEvent;

    fn name(&self) -> MetricName {
        self.name.clone()
    }

    fn attributes(&self) -> MetricAttributes {
        NumericAttributes {
            unit: None,
            higher_is_better: false,
        }
        .into()
    }

    fn update(&mut self, item: &Self::Input, _metadata: &MetricMetadata) -> SerializedEntry {
        let value = match item {
            TrainEvent::FineTuneEpoch(FineTuneSummary { loss, .. }) => *loss as f64,
            _ => return SerializedEntry::new("-".to_string(), "".to_string()),
        };
        self.state
            .update(value, 1, FormatOptions::new(self.name()).precision(4))
    }

    fn clear(&mut self) {
        self.state.reset();
    }
}

impl Numeric for FineTuneLossMetric {
    fn value(&self) -> NumericEntry {
        self.state.current_value()
    }

    fn running_value(&self) -> NumericEntry {
        self.state.running_value()
    }
}

/// Steps taken in an episode.
///
/// Tracks how many simulator steps were executed before the episode ended.
/// Shorter episodes usually mean the agent reached the goal quickly; long
/// flat lines may indicate the agent is stuck or wandering.
#[derive(Clone, Default)]
pub struct EpisodeStepsMetric {
    name: MetricName,
    state: NumericMetricState,
}

impl EpisodeStepsMetric {
    pub fn new() -> Self {
        Self {
            name: Arc::new("Episode Steps".to_string()),
            state: NumericMetricState::default(),
        }
    }
}

impl Metric for EpisodeStepsMetric {
    type Input = TrainEvent;

    fn name(&self) -> MetricName {
        self.name.clone()
    }

    fn attributes(&self) -> MetricAttributes {
        NumericAttributes {
            unit: Some("steps".to_string()),
            higher_is_better: false,
        }
        .into()
    }

    fn update(&mut self, item: &Self::Input, _metadata: &MetricMetadata) -> SerializedEntry {
        let value = match item {
            TrainEvent::Episode(EpisodeSummary { steps, .. }) => *steps as f64,
            _ => return SerializedEntry::new("-".to_string(), "".to_string()),
        };
        self.state.update(
            value,
            1,
            FormatOptions::new(self.name()).precision(1).unit("steps"),
        )
    }

    fn clear(&mut self) {
        self.state.reset();
    }
}

impl Numeric for EpisodeStepsMetric {
    fn value(&self) -> NumericEntry {
        self.state.current_value()
    }

    fn running_value(&self) -> NumericEntry {
        self.state.running_value()
    }
}

/// Completion time when the goal is reached.
///
/// Tracks the simulator time at which the episode reached the goal. Episodes
/// that do not reach the goal are reported as "-" rather than the final time.
/// Lower values mean the agent is solving the task faster.
#[derive(Clone, Default)]
pub struct CompletionTimeMetric {
    name: MetricName,
    state: NumericMetricState,
}

impl CompletionTimeMetric {
    pub fn new() -> Self {
        Self {
            name: Arc::new("Completion Time".to_string()),
            state: NumericMetricState::default(),
        }
    }
}

impl Metric for CompletionTimeMetric {
    type Input = TrainEvent;

    fn name(&self) -> MetricName {
        self.name.clone()
    }

    fn attributes(&self) -> MetricAttributes {
        NumericAttributes {
            unit: Some("s".to_string()),
            higher_is_better: false,
        }
        .into()
    }

    fn update(&mut self, item: &Self::Input, _metadata: &MetricMetadata) -> SerializedEntry {
        let value = match item {
            TrainEvent::Episode(EpisodeSummary {
                reached_goal: true,
                completion_time,
                ..
            }) => *completion_time,
            _ => return SerializedEntry::new("-".to_string(), "".to_string()),
        };
        self.state.update(
            value,
            1,
            FormatOptions::new(self.name()).precision(1).unit("s"),
        )
    }

    fn clear(&mut self) {
        self.state.reset();
    }
}

impl Numeric for CompletionTimeMetric {
    fn value(&self) -> NumericEntry {
        self.state.current_value()
    }

    fn running_value(&self) -> NumericEntry {
        self.state.running_value()
    }
}

/// Fraction of episodes that reached the goal.
///
/// Tracks the success rate of episodes. Reported as a percentage, where 100%
/// means every episode reached the goal and 0% means none did. This is the
/// simplest indicator of whether the agent is learning to solve the task.
#[derive(Clone, Default)]
pub struct GoalReachMetric {
    name: MetricName,
    state: NumericMetricState,
}

impl GoalReachMetric {
    pub fn new() -> Self {
        Self {
            name: Arc::new("Goal Reach".to_string()),
            state: NumericMetricState::default(),
        }
    }
}

impl Metric for GoalReachMetric {
    type Input = TrainEvent;

    fn name(&self) -> MetricName {
        self.name.clone()
    }

    fn attributes(&self) -> MetricAttributes {
        NumericAttributes {
            unit: Some("%".to_string()),
            higher_is_better: true,
        }
        .into()
    }

    fn update(&mut self, item: &Self::Input, _metadata: &MetricMetadata) -> SerializedEntry {
        let value = match item {
            TrainEvent::Episode(EpisodeSummary { reached_goal, .. }) => {
                if *reached_goal {
                    1.0
                } else {
                    0.0
                }
            }
            _ => return SerializedEntry::new("-".to_string(), "".to_string()),
        };
        self.state.update(
            value,
            1,
            FormatOptions::new(self.name()).precision(2).unit("%"),
        )
    }

    fn clear(&mut self) {
        self.state.reset();
    }
}

impl Numeric for GoalReachMetric {
    fn value(&self) -> NumericEntry {
        self.state.current_value()
    }

    fn running_value(&self) -> NumericEntry {
        self.state.running_value()
    }
}

/// Current epsilon-greedy exploration probability.
///
/// Tracks the probability of taking a random action instead of the policy's
/// best action. Epsilon typically decays over training, shifting the agent
/// from exploration to exploitation.
#[derive(Clone, Default)]
pub struct EpsilonMetric {
    name: MetricName,
    state: NumericMetricState,
}

impl EpsilonMetric {
    pub fn new() -> Self {
        Self {
            name: Arc::new("Epsilon".to_string()),
            state: NumericMetricState::default(),
        }
    }
}

impl Metric for EpsilonMetric {
    type Input = TrainEvent;

    fn name(&self) -> MetricName {
        self.name.clone()
    }

    fn attributes(&self) -> MetricAttributes {
        NumericAttributes {
            unit: None,
            higher_is_better: false,
        }
        .into()
    }

    fn update(&mut self, item: &Self::Input, _metadata: &MetricMetadata) -> SerializedEntry {
        let value = match item {
            TrainEvent::Episode(EpisodeSummary { epsilon, .. }) => *epsilon as f64,
            _ => return SerializedEntry::new("-".to_string(), "".to_string()),
        };
        self.state
            .update(value, 1, FormatOptions::new(self.name()).precision(4))
    }

    fn clear(&mut self) {
        self.state.reset();
    }
}

impl Numeric for EpsilonMetric {
    fn value(&self) -> NumericEntry {
        self.state.current_value()
    }

    fn running_value(&self) -> NumericEntry {
        self.state.running_value()
    }
}

/// Best completion time observed so far.
///
/// Tracks the lowest completion time seen across both normal training
/// episodes and periodic greedy evaluations. Unlike [`CompletionTimeMetric`],
/// this value is monotonically non-increasing and shows the best performance
/// achieved so far.
#[derive(Clone, Default)]
pub struct BestTimeMetric {
    name: MetricName,
    state: NumericMetricState,
}

impl BestTimeMetric {
    pub fn new() -> Self {
        Self {
            name: Arc::new("Best Time".to_string()),
            state: NumericMetricState::default(),
        }
    }
}

impl Metric for BestTimeMetric {
    type Input = TrainEvent;

    fn name(&self) -> MetricName {
        self.name.clone()
    }

    fn attributes(&self) -> MetricAttributes {
        NumericAttributes {
            unit: Some("s".to_string()),
            higher_is_better: false,
        }
        .into()
    }

    fn update(&mut self, item: &Self::Input, _metadata: &MetricMetadata) -> SerializedEntry {
        let best = match item {
            TrainEvent::Episode(EpisodeSummary { best_time, .. }) => *best_time,
            TrainEvent::GreedyEval(GreedyEvalSummary { best_time, .. }) => *best_time,
            _ => None,
        };
        let Some(value) = best else {
            return SerializedEntry::new("-".to_string(), "".to_string());
        };
        self.state.update(
            value,
            1,
            FormatOptions::new(self.name()).precision(1).unit("s"),
        )
    }

    fn clear(&mut self) {
        self.state.reset();
    }
}

impl Numeric for BestTimeMetric {
    fn value(&self) -> NumericEntry {
        self.state.current_value()
    }

    fn running_value(&self) -> NumericEntry {
        self.state.running_value()
    }
}

/// Greedy evaluation completion time.
///
/// Tracks the completion time of periodic greedy evaluations, where the agent
/// acts without exploration (`epsilon = 0`). This measures the true policy
/// quality independent of random exploration. Only successful greedy runs are
/// reported.
#[derive(Clone, Default)]
pub struct GreedyEvalTimeMetric {
    name: MetricName,
    state: NumericMetricState,
}

impl GreedyEvalTimeMetric {
    pub fn new() -> Self {
        Self {
            name: Arc::new("Greedy Eval Time".to_string()),
            state: NumericMetricState::default(),
        }
    }
}

impl Metric for GreedyEvalTimeMetric {
    type Input = TrainEvent;

    fn name(&self) -> MetricName {
        self.name.clone()
    }

    fn attributes(&self) -> MetricAttributes {
        NumericAttributes {
            unit: Some("s".to_string()),
            higher_is_better: false,
        }
        .into()
    }

    fn update(&mut self, item: &Self::Input, _metadata: &MetricMetadata) -> SerializedEntry {
        let value = match item {
            TrainEvent::GreedyEval(GreedyEvalSummary {
                reached_goal: true,
                completion_time: Some(t),
                ..
            }) => *t,
            _ => return SerializedEntry::new("-".to_string(), "".to_string()),
        };
        self.state.update(
            value,
            1,
            FormatOptions::new(self.name()).precision(1).unit("s"),
        )
    }

    fn clear(&mut self) {
        self.state.reset();
    }
}

impl Numeric for GreedyEvalTimeMetric {
    fn value(&self) -> NumericEntry {
        self.state.current_value()
    }

    fn running_value(&self) -> NumericEntry {
        self.state.running_value()
    }
}

/// Training throughput in episodes per second.
///
/// Tracks how many episodes are completed per wall-clock second. This is a
/// pure speed metric: higher values mean training is progressing faster, but
/// it does not indicate learning quality.
#[derive(Clone)]
pub struct EpisodeSpeedMetric {
    name: MetricName,
    state: NumericMetricState,
    start: Option<Instant>,
}

impl Default for EpisodeSpeedMetric {
    fn default() -> Self {
        Self::new()
    }
}

impl EpisodeSpeedMetric {
    pub fn new() -> Self {
        Self {
            name: Arc::new("Episodes/sec".to_string()),
            state: NumericMetricState::default(),
            start: None,
        }
    }
}

impl Metric for EpisodeSpeedMetric {
    type Input = TrainEvent;

    fn name(&self) -> MetricName {
        self.name.clone()
    }

    fn attributes(&self) -> MetricAttributes {
        NumericAttributes {
            unit: Some("eps/s".to_string()),
            higher_is_better: true,
        }
        .into()
    }

    fn update(&mut self, item: &Self::Input, metadata: &MetricMetadata) -> SerializedEntry {
        let episode = match item {
            TrainEvent::Episode(EpisodeSummary { episode, .. }) => *episode,
            _ => return SerializedEntry::new("-".to_string(), "".to_string()),
        };

        let now = Instant::now();
        let value = match self.start {
            None => {
                self.start = Some(now);
                0.0
            }
            Some(start) => {
                let elapsed = now.duration_since(start).as_secs_f64();
                if elapsed > 0.0 {
                    episode as f64 / elapsed
                } else {
                    0.0
                }
            }
        };

        // Use the iteration count as batch size so the running average is
        // weighted correctly.
        let batch_size = metadata.iteration.map(|i| i + 1).unwrap_or(episode);
        self.state.update(
            value,
            batch_size.max(1),
            FormatOptions::new(self.name()).precision(2).unit("eps/s"),
        )
    }

    fn clear(&mut self) {
        self.state.reset();
        self.start = None;
    }
}

impl Numeric for EpisodeSpeedMetric {
    fn value(&self) -> NumericEntry {
        self.state.current_value()
    }

    fn running_value(&self) -> NumericEntry {
        self.state.running_value()
    }
}

/// All faf-sim metrics bundled together, feeding a single Burn `MetricsRenderer`.
pub struct FafSimMetrics {
    renderer: Box<dyn MetricsRenderer>,
    loss: EpisodeLossMetric,
    fine_tune_loss: FineTuneLossMetric,
    steps: EpisodeStepsMetric,
    completion_time: CompletionTimeMetric,
    goal_reach: GoalReachMetric,
    epsilon: EpsilonMetric,
    best_time: BestTimeMetric,
    greedy_time: GreedyEvalTimeMetric,
    speed: EpisodeSpeedMetric,
}

impl FafSimMetrics {
    /// Create a metric bundle with the given renderer.
    pub fn new(renderer: Box<dyn MetricsRenderer>) -> Self {
        Self {
            renderer,
            loss: EpisodeLossMetric::new(),
            fine_tune_loss: FineTuneLossMetric::new(),
            steps: EpisodeStepsMetric::new(),
            completion_time: CompletionTimeMetric::new(),
            goal_reach: GoalReachMetric::new(),
            epsilon: EpsilonMetric::new(),
            best_time: BestTimeMetric::new(),
            greedy_time: GreedyEvalTimeMetric::new(),
            speed: EpisodeSpeedMetric::new(),
        }
    }

    /// Register every metric with the renderer.
    pub fn register(&mut self) {
        Self::register_metric(&mut *self.renderer, &self.loss);
        Self::register_metric(&mut *self.renderer, &self.fine_tune_loss);
        Self::register_metric(&mut *self.renderer, &self.steps);
        Self::register_metric(&mut *self.renderer, &self.completion_time);
        Self::register_metric(&mut *self.renderer, &self.goal_reach);
        Self::register_metric(&mut *self.renderer, &self.epsilon);
        Self::register_metric(&mut *self.renderer, &self.best_time);
        Self::register_metric(&mut *self.renderer, &self.greedy_time);
        Self::register_metric(&mut *self.renderer, &self.speed);
    }

    fn register_metric<M: Metric>(renderer: &mut dyn MetricsRenderer, metric: &M) {
        let id = MetricId::new(metric.name());
        let definition = MetricDefinition::new(id, metric);
        renderer.register_metric(definition);
    }

    /// Update all metrics with a training event and forward states to the renderer.
    pub fn update(&mut self, event: &TrainEvent, metadata: &MetricMetadata) {
        Self::update_metric(&mut *self.renderer, &mut self.loss, event, metadata);
        Self::update_metric(
            &mut *self.renderer,
            &mut self.fine_tune_loss,
            event,
            metadata,
        );
        Self::update_metric(&mut *self.renderer, &mut self.steps, event, metadata);
        Self::update_metric(
            &mut *self.renderer,
            &mut self.completion_time,
            event,
            metadata,
        );
        Self::update_metric(&mut *self.renderer, &mut self.goal_reach, event, metadata);
        Self::update_metric(&mut *self.renderer, &mut self.epsilon, event, metadata);
        Self::update_metric(&mut *self.renderer, &mut self.best_time, event, metadata);
        Self::update_metric(&mut *self.renderer, &mut self.greedy_time, event, metadata);
        Self::update_metric(&mut *self.renderer, &mut self.speed, event, metadata);
    }

    fn update_metric<M: Metric<Input = TrainEvent> + Numeric>(
        renderer: &mut dyn MetricsRenderer,
        metric: &mut M,
        event: &TrainEvent,
        metadata: &MetricMetadata,
    ) {
        let id = MetricId::new(metric.name());
        let entry = metric.update(event, metadata);
        let state = MetricState::Numeric(MetricEntry::new(id, entry), metric.value());
        renderer.update_train(state);
    }

    /// Render the current training progress.
    pub fn render(&mut self, progress: TrainingProgress, indicators: Vec<ProgressType>) {
        self.renderer.render_train(progress, indicators);
    }

    /// Notify the renderer that training ended.
    pub fn on_end(
        &mut self,
        summary: Option<LearnerSummary>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.renderer.on_train_end(summary)
    }
}

/// Helper to build a `TrainingProgress` from episode-level progress.
pub fn training_progress(
    episode: usize,
    total_episodes: usize,
    iteration: Option<usize>,
) -> TrainingProgress {
    TrainingProgress {
        progress: Some(Progress {
            items_processed: episode,
            items_total: total_episodes.max(episode),
        }),
        global_progress: Progress {
            items_processed: episode,
            items_total: total_episodes.max(episode),
        },
        iteration,
    }
}
