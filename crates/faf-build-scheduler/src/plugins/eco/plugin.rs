//! Eco scheduling mode plugin.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use crate::plugins::eco::{
    evaluate::evaluate_eco_candidates_system, generate::generate_eco_candidates_system,
};
use crate::plugins::lifecycle::SchedulerSet;

/// Plugin that registers candidate generation and evaluation for economy (mass
/// income) scheduling.
pub struct EcoSchedulingPlugin;

impl Plugin for EcoSchedulingPlugin {
    fn build(&self, app: &mut App) {
        // Register eco candidates generation/evaluation in the sets declared by
        // `SchedulerLifecyclePlugin`. The `configure_sets` call there orders
        // `GenerateCandidate -> EvaluateCandidate -> Apply` and gates the whole
        // pipeline on the `Searching` state.
        app.add_systems(
            Update,
            generate_eco_candidates_system.in_set(SchedulerSet::GenerateCandidate),
        )
        .add_systems(
            Update,
            evaluate_eco_candidates_system.in_set(SchedulerSet::EvaluateCandidate),
        );
    }
}
