use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use faf_sim::planner::mcts::train::{
    EpisodeSummary, FineTuneSummary, GreedyEvalSummary, TrainingObserver,
};

/// Event sent from the training thread to the dashboard renderer thread.
#[derive(Debug, Clone)]
pub(crate) enum DashboardEvent {
    Episode(EpisodeSummary),
    GreedyEval(GreedyEvalSummary),
    FineTuneEpoch(FineTuneSummary),
}

/// [`TrainingObserver`] implementation that forwards events to the dashboard.
pub struct DashboardObserver {
    sender: Sender<DashboardEvent>,
    stop_flag: Arc<AtomicBool>,
}

impl DashboardObserver {
    pub(crate) fn new(sender: Sender<DashboardEvent>, stop_flag: Arc<AtomicBool>) -> Self {
        Self { sender, stop_flag }
    }
}

impl TrainingObserver for DashboardObserver {
    fn on_episode_end(&mut self, summary: EpisodeSummary) {
        let _ = self.sender.send(DashboardEvent::Episode(summary));
    }

    fn on_greedy_eval(&mut self, summary: GreedyEvalSummary) {
        let _ = self.sender.send(DashboardEvent::GreedyEval(summary));
    }

    fn on_fine_tune_epoch(&mut self, summary: FineTuneSummary) {
        let _ = self.sender.send(DashboardEvent::FineTuneEpoch(summary));
    }

    fn should_stop(&self) -> bool {
        self.stop_flag.load(Ordering::Relaxed)
    }
}
