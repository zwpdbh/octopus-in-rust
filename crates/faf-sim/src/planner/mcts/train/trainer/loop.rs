//! Trainer for the hierarchical policy networks.

use super::super::config::TrainStats;
use super::super::episode::{BuildTrajectory, TrajectoryStep};
use super::super::math::format_time;
use super::super::observer::{EpisodeSummary, GreedyEvalSummary};
use crate::planner::core::{Goal, PlannerConfig};
use crate::planner::mcts::selections::PlanEdgeIndex;
use crate::units::Units;

use super::Trainer;

impl Trainer {
    /// Train the policy on the given goal.
    pub fn train(&mut self, units: &Units, goal: &Goal) -> TrainStats {
        let plan = units.plan_graph(*goal);
        let edge_index = PlanEdgeIndex::new(&plan);
        let planner_config = PlannerConfig::default();
        let mut stats = TrainStats::default();

        let mut best_time: Option<f64> = if let Some(ref model) = self.best_model {
            let baseline = Trainer::evaluate_greedy_with_model(
                model,
                units,
                goal,
                &plan,
                &edge_index,
                &planner_config,
                self.config.max_steps,
                self.config.dt,
                &self.device,
            );
            if self.config.verbose {
                if let Some(t) = baseline {
                    eprintln!("Resumed model greedy baseline: {}", format_time(t, true));
                } else {
                    eprintln!("Resumed model did not reach the goal in a greedy evaluation.");
                }
            }
            baseline
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
                    if self.config.verbose {
                        eprintln!("No improvement for {} episodes; stopping early.", patience);
                    }
                    break;
                }
            }

            let epsilon = self.current_epsilon(ep);
            let episode = self.run_episode(
                ep,
                units,
                goal,
                &plan,
                &edge_index,
                &planner_config,
                epsilon,
            );

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
                                edge_index: s.edge_index,
                                target_power: s.target_power,
                                desired_squad: s.desired_squad,
                                shortfall: s.shortfall,
                                upgrade_index: s.upgrade_index,
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
                if self.config.verbose {
                    eprintln!("  greedy eval at ep={}: running...", ep + 1);
                }
                if let Some(greedy_time) =
                    self.evaluate_greedy(units, goal, &plan, &edge_index, &planner_config)
                {
                    let is_new_best = best_time.map_or(true, |t| greedy_time < t);
                    if is_new_best {
                        best_time = Some(greedy_time);
                        episodes_since_best = 0;
                        self.best_model = Some(self.model.clone());
                        self.best_trajectory = None;
                    }
                    if self.config.verbose {
                        eprintln!(
                            "  greedy eval at ep={}: time={} best={}",
                            ep + 1,
                            format_time(greedy_time, true),
                            format_time(best_time.unwrap_or(0.0), best_time.is_some())
                        );
                    }
                    if let Some(ref mut observer) = self.observer {
                        observer.on_greedy_eval(GreedyEvalSummary {
                            episode: ep + 1,
                            reached_goal: true,
                            completion_time: Some(greedy_time),
                            best_time,
                        });
                    }
                } else {
                    if self.config.verbose {
                        eprintln!("  greedy eval at ep={}: did not reach goal", ep + 1);
                    }
                    if let Some(ref mut observer) = self.observer {
                        observer.on_greedy_eval(GreedyEvalSummary {
                            episode: ep + 1,
                            reached_goal: false,
                            completion_time: None,
                            best_time,
                        });
                    }
                }
            }

            if let Some(ref mut observer) = self.observer {
                observer.on_episode_end(EpisodeSummary {
                    episode: ep + 1,
                    total_episodes: self.config.episodes,
                    steps: episode.steps.len(),
                    epsilon,
                    reached_goal: episode.reached_goal,
                    completion_time: episode.completion_time,
                    loss,
                    best_time,
                });
                if self.should_stop() {
                    if self.config.verbose {
                        eprintln!("Stop requested; exiting training loop.");
                    }
                    break;
                }
            }

            if self.config.verbose {
                let time_str = format_time(episode.completion_time, episode.reached_goal);
                let best_str = format_time(best_time.unwrap_or(0.0), best_time.is_some());
                let loss_str = loss
                    .map(|l| format!("{:.4}", l))
                    .unwrap_or_else(|| "-".to_string());
                eprintln!(
                    "ep={:>4} steps={:>4} eps={:.4} reached={:>5} time={:>14} best={:>14} loss={:>10}",
                    ep + 1,
                    episode.steps.len(),
                    epsilon,
                    episode.reached_goal,
                    time_str,
                    best_str,
                    loss_str
                );
            }

            ep += 1;
            episodes_since_best += 1;

            if target_hit {
                if self.config.verbose {
                    eprintln!("Target completion time reached; stopping early.");
                }
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
