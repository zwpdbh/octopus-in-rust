#![allow(unused)]
use std::collections::HashMap;

use bevy_ecs::prelude::*;
use uuid::Uuid;

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
    pub fn net_mass_rate(&mut self) -> f64 {
        (self.mass_generate_rate - self.mass_drain)
    }

    pub fn net_energy_rate(&self) -> f64 {
        (self.energy_generate_rate - self.energy_drain)
    }

    pub fn energy_efficiency(&self) -> f64 {
        if self.energy_in_storage > 0.0 {
            1.0
        } else {
            self.energy_generate_rate / self.energy_drain
        }
    }

    // need to consider energy_efficiency
    fn mass_efficiency(&self) -> f64 {
        if self.mass_in_storage > 0.0 {
            1.0
        } else {
            self.mass_generate_rate / self.mass_drain
        }
    }

    // direct efficiency apply to update build progress
    pub fn construction_efficiency(&self) -> f64 {
        self.mass_efficiency() * self.energy_efficiency()
    }
}

type BuildPowerAssigned = f64;
type BuildTimeConstructed = f64;
type BuildTimeNeeded = f64;
type TaskId = Uuid;
type MassDrain = f64;
type EnergyDrain = f64;
type BuildTime = f64;

#[derive(Resource)]
pub struct Constructions {
    pub assigned_bp: HashMap<TaskId, (MassDrain, EnergyDrain, BuildTime, BuildPowerAssigned)>,
    pub progress: HashMap<TaskId, BuildTimeConstructed>,
}
