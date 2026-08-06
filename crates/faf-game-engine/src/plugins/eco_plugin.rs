#![allow(unused)]
use crate::resources::PlayerEco;
use crate::systems::player_eco_systems;
use bevy_app::prelude::*;
use bevy_ecs::schedule::IntoScheduleConfigs;

/// EcoPlugin should prepare all system need for running engine for doing construction
struct EcoPlugin;

impl Plugin for EcoPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                // player_eco_systems::update_player_eco_from_existing_units,
                player_eco_systems::update_player_eco_from_building_units,
                player_eco_systems::update_construction_pragress,
                player_eco_systems::apply_finished_constructions,
            )
                .chain(),
        );
    }
}
