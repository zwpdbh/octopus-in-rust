#![allow(unused)]
use std::collections::HashMap;

use bevy_ecs::prelude::*;

pub struct TotalMassProduction {
    rate: usize,
}

pub struct TotalEnergyProduction {
    rate: usize,
}

pub struct TotalMassDrain {
    rate: usize,
}

pub struct TotalEnergyDrain {
    rate: usize,
}

pub struct MassStorage {
    pub mass_in_storage: usize,
    pub capacity: usize,
}

pub struct EnergyStorage {
    pub capacity: usize,
    pub energy_in_storage: usize,
}

pub struct Building {
    tasks: HashMap<Entity, Vec<Entity>>,
}
