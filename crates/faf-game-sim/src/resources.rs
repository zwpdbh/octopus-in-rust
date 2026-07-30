use bevy_ecs::prelude::*;

#[derive(Resource)]
pub struct PlayerEcoState {
    pub mass_production_rate: usize,
    pub mass_drain_rate: usize,
    pub energy_production_rate: usize,
    pub energy_drain_rate: usize,
    pub mass_in_storage: usize,
    pub energy_in_storage: usize,
    pub mass_capacity: usize,
    pub energy_capacity: usize,
}
