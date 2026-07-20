//! Greedy best-first scheduling algorithm.

use bevy_app::prelude::*;

use crate::algorithms::SchedulingAlgorithm;
use crate::plugins::greedy::GreedyPlugin;

/// Greedy search: at each iteration, generate candidates, simulate them, and
/// commit the lowest-scoring candidate.
#[derive(Debug, Default, Clone, Copy)]
pub struct Greedy;

impl SchedulingAlgorithm for Greedy {
    fn name(&self) -> &'static str {
        "greedy"
    }

    fn configure_app(&self, app: &mut App) {
        app.add_plugins(GreedyPlugin);
    }
}
