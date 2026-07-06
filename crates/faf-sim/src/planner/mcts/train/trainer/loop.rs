//! Trainer main loop for the direction-only policy network.

use super::super::config::TrainStats;
use super::super::episode::{BuildTrajectory, Episode, TrajectoryStep};
use super::super::metric::metrics::training_progress;
use super::super::metric::{EpisodeSummary, GreedyEvalSummary, TrainEvent};
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
        let plan = build_plan_graph(units, *goal);
        let mut stats = TrainStats::default();

        self.register_metrics();

        let mut best_time = self.initial_best_time(units, goal, &planner_config);
        let mut ep = 0usize;

        loop {
            if self.should_stop_training(ep) {
                break;
            }

            let epsilon = self.current_epsilon(ep);
            let episode = self.run_episode(units, goal, &planner_config, epsilon, &plan);
            let loss = self.update_policy(&episode, &mut stats);
            stats.episode_lengths.push(episode.steps.len());

            let target_hit = episode
                .reached_goal
                .then(|| self.handle_goal_reached(&episode, &mut stats, &mut best_time))
                .unwrap_or(false);

            self.maybe_evaluate_greedy(units, goal, &planner_config, ep + 1, &mut best_time);
            self.emit_episode_metrics(ep + 1, &episode, epsilon, loss, best_time);

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

    /// Compute the initial best time from a pre-existing best model.
    ///
    /// When training from scratch this is `None`; when resuming from a
    /// checkpoint it evaluates the loaded model greedily to establish a
    /// baseline.
    fn initial_best_time(
        &self,
        units: &Units,
        goal: &Goal,
        planner_config: &PlannerConfig,
    ) -> Option<f64> {
        self.best_model.as_ref().and_then(|model| {
            Trainer::evaluate_greedy_with_model(
                model,
                units,
                goal,
                planner_config,
                self.config.max_steps,
                self.config.dt,
                &self.device,
            )
        })
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

    /// Update training statistics and the best model when an episode reaches the goal.
    ///
    /// Returns `true` if the target completion time was hit.
    fn handle_goal_reached(
        &mut self,
        episode: &Episode,
        stats: &mut TrainStats,
        best_time: &mut Option<f64>,
    ) -> bool {
        stats.goal_reaches += 1;
        stats.completion_times.push(episode.completion_time);

        let is_new_best = best_time.is_none_or(|t| episode.completion_time < t);
        if is_new_best {
            *best_time = Some(episode.completion_time);
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

    /// Run a periodic greedy evaluation and update the best model if it improves.
    fn maybe_evaluate_greedy(
        &mut self,
        units: &Units,
        goal: &Goal,
        planner_config: &PlannerConfig,
        episode: usize,
        best_time: &mut Option<f64>,
    ) {
        let interval = self.config.greedy_eval_interval;
        if interval == 0 || episode == 0 || !episode.is_multiple_of(interval) {
            return;
        }

        let greedy_time = self.evaluate_greedy(units, goal, planner_config);

        if let Some(greedy_time) = greedy_time {
            let is_new_best = best_time.is_none_or(|t| greedy_time < t);
            if is_new_best {
                *best_time = Some(greedy_time);
                self.best_model = Some(self.model.clone());
                self.best_trajectory = None;
            }
        }

        self.emit_greedy_eval_metrics(episode, greedy_time.is_some(), greedy_time, *best_time);
    }

    /// Emit an `Episode` event to the metrics renderer.
    fn emit_episode_metrics(
        &mut self,
        episode: usize,
        summary: &Episode,
        epsilon: f32,
        loss: Option<f32>,
        best_time: Option<f64>,
    ) {
        let Some(ref mut metrics) = self.metrics else {
            return;
        };

        let metadata = metric_metadata(episode, self.config.episodes);
        metrics.update(
            &TrainEvent::Episode(EpisodeSummary {
                episode,
                total_episodes: self.config.episodes,
                steps: summary.steps.len(),
                epsilon,
                reached_goal: summary.reached_goal,
                completion_time: summary.completion_time,
                loss,
                best_time,
            }),
            &metadata,
        );
        metrics.render(
            training_progress(episode, self.config.episodes, Some(episode)),
            vec![],
        );
    }

    /// Emit a `GreedyEval` event to the metrics renderer.
    fn emit_greedy_eval_metrics(
        &mut self,
        episode: usize,
        reached_goal: bool,
        completion_time: Option<f64>,
        best_time: Option<f64>,
    ) {
        let Some(ref mut metrics) = self.metrics else {
            return;
        };

        let metadata = metric_metadata(episode, self.config.episodes);
        metrics.update(
            &TrainEvent::GreedyEval(GreedyEvalSummary {
                episode,
                reached_goal,
                completion_time,
                best_time,
            }),
            &metadata,
        );
        metrics.render(
            training_progress(episode, self.config.episodes, Some(episode)),
            vec![],
        );
    }

    pub(crate) fn current_epsilon(&self, ep: usize) -> f32 {
        let decay = self.config.epsilon_decay_episodes;
        if decay == 0 {
            return self.config.epsilon;
        }
        if ep >= decay {
            return self.config.epsilon_final;
        }
        let progress = ep as f32 / decay as f32;
        self.config.epsilon - (self.config.epsilon - self.config.epsilon_final) * progress
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
