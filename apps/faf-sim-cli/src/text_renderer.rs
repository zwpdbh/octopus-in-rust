//! Plain-text `MetricsRenderer` for non-TUI training runs.
//!
//! This prints the same per-episode line that the old verbose trainer emitted,
//! so `--text` output remains familiar.

use std::collections::HashMap;

use faf_sim::planner::mcts::train::{
    EvaluationName, EvaluationProgress, LearnerSummary, MetricDefinition, MetricId, MetricState,
    MetricsRenderer, MetricsRendererEvaluation, MetricsRendererTraining, NumericEntry,
    ProgressType, TrainingProgress,
};

const LOSS: &str = "Episode Loss";
const STEPS: &str = "Episode Steps";
const EPSILON: &str = "Epsilon";
const GOAL_REACH: &str = "Goal Reach";
const COMPLETION_TIME: &str = "Completion Time";
const BEST_TIME: &str = "Best Time";

/// Plain-text renderer that prints per-episode progress to stderr.
pub struct TextMetricsRenderer {
    names: HashMap<MetricId, String>,
    values: HashMap<String, String>,
    quiet: bool,
}

impl TextMetricsRenderer {
    /// Create a new text renderer.
    pub fn new() -> Self {
        Self {
            names: HashMap::new(),
            values: HashMap::new(),
            quiet: false,
        }
    }

    /// Create a renderer that consumes metric updates but produces no output.
    /// Useful for `--quiet` training runs.
    pub fn quiet() -> Self {
        Self {
            names: HashMap::new(),
            values: HashMap::new(),
            quiet: true,
        }
    }

    fn current_value(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(|s| s.as_str())
    }

    fn numeric_value(state: &MetricState) -> Option<f64> {
        match state {
            MetricState::Numeric(_, NumericEntry::Value(v)) => Some(*v),
            MetricState::Numeric(
                _,
                NumericEntry::Aggregated {
                    aggregated_value, ..
                },
            ) => Some(*aggregated_value),
            MetricState::Generic(_) => None,
        }
    }
}

impl MetricsRendererTraining for TextMetricsRenderer {
    fn update_train(&mut self, state: MetricState) {
        let id = match &state {
            MetricState::Generic(entry) => &entry.metric_id,
            MetricState::Numeric(entry, _) => &entry.metric_id,
        };

        if let Some(name) = self.names.get(id).cloned() {
            let formatted = if let Some(value) = Self::numeric_value(&state) {
                match name.as_str() {
                    LOSS => format!("{:.4}", value),
                    EPSILON => format!("{:.4}", value),
                    STEPS => format!("{:.0}", value),
                    GOAL_REACH => {
                        if value > 0.5 {
                            "true".to_string()
                        } else {
                            "false".to_string()
                        }
                    }
                    COMPLETION_TIME | BEST_TIME => {
                        if value.is_finite() && value > 0.0 {
                            format_time(value)
                        } else {
                            "-".to_string()
                        }
                    }
                    _ => format!("{:.4}", value),
                }
            } else {
                "-".to_string()
            };
            self.values.insert(name, formatted);
        }
    }

    fn update_valid(&mut self, _state: MetricState) {}

    fn render_train(&mut self, item: TrainingProgress, _progress_indicators: Vec<ProgressType>) {
        if self.quiet {
            return;
        }
        // Only print a line when an episode has been observed (i.e. loss is present).
        if !self.values.contains_key(LOSS) {
            return;
        }

        let episode = item
            .iteration
            .or_else(|| item.progress.map(|p| p.items_processed))
            .unwrap_or(0);
        let steps = self.current_value(STEPS).unwrap_or("-");
        let eps = self.current_value(EPSILON).unwrap_or("-");
        let reached = self.current_value(GOAL_REACH).unwrap_or("false");
        let time = self.current_value(COMPLETION_TIME).unwrap_or("-");
        let best = self.current_value(BEST_TIME).unwrap_or("-");
        let loss = self.current_value(LOSS).unwrap_or("-");

        eprintln!(
            "ep={:>4} steps={:>4} eps={:>6} reached={:>5} time={:>14} best={:>14} loss={:>10}",
            episode, steps, eps, reached, time, best, loss
        );

        // Clear so that metrics which are not updated in the next event print "-".
        self.values.clear();
    }

    fn render_valid(&mut self, item: TrainingProgress, _progress_indicators: Vec<ProgressType>) {
        if self.quiet {
            return;
        }
        eprintln!("{item:?}");
    }

    fn on_train_end(
        &mut self,
        summary: Option<LearnerSummary>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.quiet {
            return Ok(());
        }
        if let Some(summary) = summary {
            println!("{summary}");
        }
        Ok(())
    }
}

impl MetricsRendererEvaluation for TextMetricsRenderer {
    fn update_test(&mut self, _name: EvaluationName, _state: MetricState) {}
    fn render_test(&mut self, item: EvaluationProgress, _progress_indicators: Vec<ProgressType>) {
        eprintln!("{item:?}");
    }
}

impl MetricsRenderer for TextMetricsRenderer {
    fn manual_close(&mut self) {}

    fn register_metric(&mut self, definition: MetricDefinition) {
        self.names.insert(definition.metric_id, definition.name);
    }
}

fn format_time(seconds: f64) -> String {
    let minutes = (seconds / 60.0).floor();
    let secs = seconds - minutes * 60.0;
    format!("{:.0}m {:.1}s", minutes, secs)
}
