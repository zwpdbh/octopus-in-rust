//! Unit scheduling mode plugin.

pub mod evaluate;
pub mod generate;

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use crate::plugins::apply::apply_best_system;
use crate::plugins::lifecycle::SchedulerSet;
use crate::plugins::unit::{
    evaluate::evaluate_unit_candidates_system, generate::generate_unit_candidates_system,
};

/// Plugin that registers the full unit scheduling lifecycle.
pub struct UnitSchedulingPlugin;

impl Plugin for UnitSchedulingPlugin {
    fn build(&self, app: &mut App) {
        // Register unit candidate generation/evaluation in the sets declared by
        // `SchedulerLifecyclePlugin`. The global pipeline order and state gate are
        // configured there, not here.
        app.add_systems(
            Update,
            generate_unit_candidates_system.in_set(SchedulerSet::GenerateCandidate),
        )
        .add_systems(
            Update,
            evaluate_unit_candidates_system.in_set(SchedulerSet::EvaluateCandidate),
        )
        .add_systems(Update, apply_best_system.in_set(SchedulerSet::Apply));
    }
}
