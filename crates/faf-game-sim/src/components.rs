#![allow(unused)]

use bevy_ecs::prelude::*;
use uuid::Uuid;
#[derive(Component)]
pub struct Unit {
    id: Uuid,
    name: String,
    mass: usize,
    energy: usize,
    build_time: usize,
    tech_level: UnitTechLevel,
    build_power: Option<usize>,
}

#[derive(Component)]
pub struct Building {
    target: Uuid,
}

#[derive(Component)]
pub struct BuiltBy {
    builders: Vec<Uuid>,
    finished_in_seconds: usize,
}

#[derive(Component)]
pub enum UnitTechLevel {
    T1,
    T2,
    T3,
    T4,
}

#[derive(Component)]
pub struct GenerateMass {
    rate: usize,
}

#[derive(Component)]
pub struct GeneratePower {
    rate: usize,
}

#[derive(Component)]
pub struct ProvideMassStorage {
    capacity: usize,
}

#[derive(Component)]
pub struct ProvideEnergyStorage {
    capacity: usize,
}
