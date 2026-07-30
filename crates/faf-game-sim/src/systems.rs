#![allow(unused)]
use super::components::*;
use super::resources::*;
use bevy_ecs::prelude::*;

// how to model construction in FA?

pub fn new_round_system(mut player_eco: ResMut<PlayerEcoState>) {
    player_eco.mass_production_rate = 1;
    player_eco.energy_production_rate = 20;
    player_eco.mass_capacity = 650;
    player_eco.mass_in_storage = 650;
    player_eco.energy_capacity = 4000;
    player_eco.energy_in_storage = 4000;
    player_eco.mass_drain_rate = 0;
    player_eco.energy_drain_rate = 0;
}

pub fn create_build_task_system(mut query: Query<&mut Building>) {
    todo!()
}

pub fn update_build_progress_system(
    mut player_eco: ResMut<PlayerEcoState>,
    query: Query<&mut BuiltBy>,
) {
    todo!()
}

pub fn check_build_progress_system(
    mut player_eco: ResMut<PlayerEcoState>,
    mut query: Query<&mut BuiltBy>,
) {
    todo!()
}
