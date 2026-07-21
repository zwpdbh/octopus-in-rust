//! Scheduling algorithm abstraction and registry.

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use bevy_app::App;

pub mod greedy;
pub mod heuristic;
pub use greedy::Greedy;

/// Selectable scheduling algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize)]
pub enum AlgorithmKind {
    /// Greedy best-first search.
    ///
    /// This is the default algorithm.
    Greedy,
}

/// A scheduling algorithm that can plan eco or unit targets.
pub trait SchedulingAlgorithm: Send + Sync {
    fn name(&self) -> &'static str;

    /// Add this algorithm's systems to a scheduler app.
    ///
    /// The app already contains the shared search state and a scheduling-mode
    /// plugin; this method only registers algorithm-specific systems (such as
    /// the greedy selection system).
    fn configure_app(&self, app: &mut App);
}

/// Instantiate the algorithm identified by `kind`.
pub fn algorithm_by_kind(kind: AlgorithmKind) -> Box<dyn SchedulingAlgorithm> {
    match kind {
        AlgorithmKind::Greedy => Box::new(Greedy),
    }
}
