#![allow(unused)]
use std::collections::HashMap;

use bevy_ecs::prelude::*;

#[derive(Resource)]
pub struct PlayerEco {
    // mass produce vs consume
    pub mass_generate_rate: f64,
    pub mass_drain: f64,

    // energy produce vs consume
    pub energy_generate_rate: f64,
    pub energy_drain: f64,

    // storage related
    pub mass_in_storage: f64,
    pub max_capacity_in_mass_storage: f64,
    pub energy_in_storage: f64,
    pub max_capacity_in_energy_storage: f64,
}

impl PlayerEco {
    fn net_mass_rate(&mut self) -> f64 {
        (self.mass_generate_rate - self.mass_drain)
    }

    fn net_energy_rate(&self) -> f64 {
        (self.energy_generate_rate - self.energy_drain)
    }

    pub fn update_storage(&mut self) {
        let net_mass_rate = self.net_mass_rate();
        let net_energy_rate = self.net_energy_rate();

        self.mass_in_storage = self
            .max_capacity_in_mass_storage
            .min(self.mass_in_storage + self.net_mass_rate());

        self.energy_in_storage = self
            .max_capacity_in_energy_storage
            .min(self.energy_in_storage + self.net_energy_rate())
    }
}
