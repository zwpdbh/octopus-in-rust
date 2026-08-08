use crate::systems::player_eco_systems;
use crate::systems::time_systems;
use bevy_app::prelude::*;
use bevy_ecs::schedule::IntoScheduleConfigs;

/// Plugin that sets up the core economy and construction systems.
///
/// The engine is tick-based: each `app.update()` advances the simulation by
/// exactly one tick (one simulation second).  Real-world playback speed is
/// not handled here; the caller (`faf-sim-service`) controls that by
/// throttling how often it calls `app.update()`.
pub struct EcoPlugin;

impl Plugin for EcoPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                // Aggregate drains from active construction tasks and update
                // player storage / generation.
                player_eco_systems::update_player_eco_from_building_units,
                // Advance each ConstructionTarget by the total build power
                // assigned to its task.
                player_eco_systems::update_construction_pragress,
                // Clean up finished builders/targets and apply the completed
                // unit's eco effects.
                player_eco_systems::apply_finished_constructions,
                // Advance the simulation clock by one tick.
                time_systems::advance_time,
            )
                .chain(),
        );
    }
}
