//! Trainer main loop for the direction-only policy network.

use super::super::config::TrainStats;
use super::super::episode::{BuildTrajectory, TrajectoryStep};
use super::super::metric::metrics::training_progress;
use super::super::metric::{EpisodeSummary, GreedyEvalSummary, TrainEvent};
use crate::planner::core::{Goal, PlannerConfig};
use crate::units::Units;
use burn::data::dataloader::Progress;
use burn::train::metric::MetricMetadata;

use super::Trainer;

impl Trainer {
    /// Train the policy on the given goal.
    pub fn train(&mut self, units: &Units, goal: &Goal) -> TrainStats {
        let planner_config = PlannerConfig::default();
        let mut stats = TrainStats::default();

        if let Some(ref mut metrics) = self.metrics {
            metrics.register();
        }

        let mut best_time: Option<f64> = if let Some(ref model) = self.best_model {
            Trainer::evaluate_greedy_with_model(
                model,
                units,
                goal,
                &planner_config,
                self.config.max_steps,
                self.config.dt,
                &self.device,
            )
        } else {
            None
        };

        let mut ep = 0usize;
        let mut episodes_since_best = 0usize;
        loop {
            if self.config.episodes != 0 && ep >= self.config.episodes {
                break;
            }

            if let Some(patience) = self.config.patience {
                if best_time.is_some() && episodes_since_best >= patience {
                    break;
                }
            }

            let epsilon = self.current_epsilon(ep);
            let episode = self.run_episode(ep, units, goal, &planner_config, epsilon);

            let loss = if !episode.steps.is_empty() {
                let loss = self.update(&episode);
                stats.losses.push(loss);
                Some(loss)
            } else {
                None
            };

            stats.episode_lengths.push(episode.steps.len());
            let mut target_hit = false;
            if episode.reached_goal {
                stats.goal_reaches += 1;
                stats.completion_times.push(episode.completion_time);
                let is_new_best = best_time.map_or(true, |t| episode.completion_time < t);
                if is_new_best {
                    best_time = Some(episode.completion_time);
                    episodes_since_best = 0;
                    self.best_model = Some(self.model.clone());
                    self.best_trajectory = Some(BuildTrajectory {
                        steps: episode
                            .steps
                            .iter()
                            .map(|s| TrajectoryStep {
                                direction_index: s.direction_index,
                                shortfall: s.shortfall,
                            })
                            .collect(),
                    });
                }
                if let Some(target) = self.config.target_time {
                    if episode.completion_time <= target {
                        target_hit = true;
                    }
                }
            }

            let interval = self.config.greedy_eval_interval;
            if interval > 0 && ep > 0 && (ep + 1) % interval == 0 {
                if let Some(greedy_time) = self.evaluate_greedy(units, goal, &planner_config) {
                    let is_new_best = best_time.map_or(true, |t| greedy_time < t);
                    if is_new_best {
                        best_time = Some(greedy_time);
                        episodes_since_best = 0;
                        self.best_model = Some(self.model.clone());
                        self.best_trajectory = None;
                    }
                    if let Some(ref mut metrics) = self.metrics {
                        let metadata = metric_metadata(ep + 1, self.config.episodes);
                        metrics.update(
                            &TrainEvent::GreedyEval(GreedyEvalSummary {
                                episode: ep + 1,
                                reached_goal: true,
                                completion_time: Some(greedy_time),
                                best_time,
                            }),
                            &metadata,
                        );
                        metrics.render(
                            training_progress(ep + 1, self.config.episodes, Some(ep + 1)),
                            vec![],
                        );
                    }
                } else if let Some(ref mut metrics) = self.metrics {
                    let metadata = metric_metadata(ep + 1, self.config.episodes);
                    metrics.update(
                        &TrainEvent::GreedyEval(GreedyEvalSummary {
                            episode: ep + 1,
                            reached_goal: false,
                            completion_time: None,
                            best_time,
                        }),
                        &metadata,
                    );
                    metrics.render(
                        training_progress(ep + 1, self.config.episodes, Some(ep + 1)),
                        vec![],
                    );
                }
            }

            if let Some(ref mut metrics) = self.metrics {
                let metadata = metric_metadata(ep + 1, self.config.episodes);
                metrics.update(
                    &TrainEvent::Episode(EpisodeSummary {
                        episode: ep + 1,
                        total_episodes: self.config.episodes,
                        steps: episode.steps.len(),
                        epsilon,
                        reached_goal: episode.reached_goal,
                        completion_time: episode.completion_time,
                        loss,
                        best_time,
                    }),
                    &metadata,
                );
                metrics.render(
                    training_progress(ep + 1, self.config.episodes, Some(ep + 1)),
                    vec![],
                );
                if self.should_stop() {
                    break;
                }
            }

            ep += 1;
            episodes_since_best += 1;

            if target_hit {
                break;
            }
        }

        stats
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
