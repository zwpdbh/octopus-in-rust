//! Eco scheduling mode plugin.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use crate::plugins::decide_direction::decide_eco_direction_system;
use crate::plugins::eco::{
    evaluate::evaluate_eco_candidates_system, generate::generate_eco_candidates_system,
};
use crate::plugins::lifecycle::SchedulerSet;
use crate::plugins::observe::observe_eco_system;

/// Plugin that registers candidate generation and evaluation for economy (mass
/// income) scheduling.
pub struct EcoSchedulingPlugin;

impl Plugin for EcoSchedulingPlugin {
    fn build(&self, app: &mut App) {
        // Register the eco pipeline in the sets declared by
        // `SchedulerLifecyclePlugin`. The global order is:
        //
        //     Observe -> DecideDirection -> GenerateCandidate -> EvaluateCandidate -> Apply
        app.add_systems(Update, observe_eco_system.in_set(SchedulerSet::Observe))
            .add_systems(
                Update,
                decide_eco_direction_system.in_set(SchedulerSet::DecideDirection),
            )
            .add_systems(
                Update,
                generate_eco_candidates_system.in_set(SchedulerSet::GenerateCandidate),
            )
            .add_systems(
                Update,
                evaluate_eco_candidates_system.in_set(SchedulerSet::EvaluateCandidate),
            );
    }
}
