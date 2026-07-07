//! Trainer main loop for the direction-only policy network.

use super::super::config::TrainStats;
use super::super::episode::{BuildTrajectory, Episode, TrajectoryStep};
use super::super::metric::metrics::training_progress;
use super::super::metric::{EpisodeSummary, TrainEvent};
use std::rc::Rc;

use crate::planner::core::{Goal, PlannerConfig};
use crate::planner::plan_graph::build_plan_graph;
use crate::units::Units;
use burn::data::dataloader::Progress;
use burn::train::metric::MetricMetadata;

use super::Trainer;

impl Trainer {
    /// Train the policy on the given goal.
    pub fn train(&mut self, units: &Units, goal: &Goal) -> TrainStats {
        let planner_config = PlannerConfig {
            max_mex_count: self.config.max_mex_count,
            ..PlannerConfig::default()
        };
        if self.plan.is_none() {
            self.plan = Some(Rc::new(build_plan_graph(units, *goal)));
        }
        let plan = Rc::clone(self.plan.as_ref().expect("plan graph just initialized"));
        let mut stats = TrainStats::default();

        self.register_metrics();

        let mut ep = 0usize;

        loop {
            if self.should_stop_training(ep) {
                break;
            }

            let episode = self.run_episode(units, goal, &planner_config, &plan);
            let loss = self.update_policy(&episode, &mut stats);
            stats.episode_lengths.push(episode.steps.len());

            let target_hit = if episode.reached_goal {
                self.handle_goal_reached(&episode, &mut stats)
            } else {
                false
            };

            self.emit_episode_metrics(ep + 1, &episode, loss);

            ep += 1;

            if target_hit {
                break;
            }
        }

        stats
    }

    /// Register all metrics with the renderer if metrics are configured.
    fn register_metrics(&mut self) {
        if let Some(ref mut metrics) = self.metrics {
            metrics.register();
        }
    }

    /// Check whether training should stop before starting episode `ep`.
    ///
    /// Training stops when either the episode limit is reached or the user
    /// requests a stop through the interrupter / stop flag.
    fn should_stop_training(&self, ep: usize) -> bool {
        if self.config.episodes != 0 && ep >= self.config.episodes {
            return true;
        }
        self.should_stop()
    }

    /// Run one policy-gradient update if the episode produced any steps.
    fn update_policy(&mut self, episode: &Episode, stats: &mut TrainStats) -> Option<f32> {
        if episode.steps.is_empty() {
            return None;
        }

        let loss = self.update(episode);
        stats.losses.push(loss);
        Some(loss)
    }

    /// Update training statistics, best trajectory, and best model when an
    /// episode reaches the goal.
    ///
    /// The saved model is updated whenever a training episode achieves a new
    /// best completion time, so that `simulate` receives a model that has
    /// demonstrably reached the goal. The best trajectory is retained for
    /// supervised fine-tuning.
    /// Returns `true` if the target completion time was hit.
    fn handle_goal_reached(&mut self, episode: &Episode, stats: &mut TrainStats) -> bool {
        stats.goal_reaches += 1;
        stats.completion_times.push(episode.completion_time);

        let is_new_best = self
            .best_train_time
            .is_none_or(|t| episode.completion_time < t);
        if is_new_best {
            self.best_train_time = Some(episode.completion_time);
            self.best_model = Some(self.model.clone());
            self.best_trajectory = Some(BuildTrajectory {
                steps: episode
                    .steps
                    .iter()
                    .map(|s| TrajectoryStep {
                        direction_index: s.direction_index,
                    })
                    .collect(),
            });
        }

        self.config
            .target_time
            .is_some_and(|target| episode.completion_time <= target)
    }

    /// Emit an `Episode` event to the metrics renderer.
    fn emit_episode_metrics(&mut self, episode: usize, summary: &Episode, loss: Option<f32>) {
        let Some(ref mut metrics) = self.metrics else {
            return;
        };

        let metadata = metric_metadata(episode, self.config.episodes);
        metrics.update(
            &TrainEvent::Episode(EpisodeSummary {
                episode,
                total_episodes: self.config.episodes,
                steps: summary.steps.len(),
                reached_goal: summary.reached_goal,
                completion_time: summary.completion_time,
                loss,
            }),
            &metadata,
        );
        metrics.render(
            training_progress(episode, self.config.episodes, Some(episode)),
            vec![],
        );
    }
}

fn metric_metadata(episode: usize, total_episodes: usize) -> MetricMetadata {
    let progress = Progress {
        items_processed: episode,
        items_total: total_episodes.max(episode),
    };
    MetricMetadata {
        progress: progress.clone(),
        global_progress: progress,
        iteration: Some(episode),
        lr: None,
    }
}
