#![allow(unused)]

use bevy_ecs::prelude::*;
use uuid::Uuid;
#[derive(Component)]
pub struct Unit {
    id: Uuid,
    name: String,
    nick_name: Option<String>,
}

struct UnitCost {
    mass: usize,
    energy: usize,
    build_time: usize,
}

struct BuildPower {
    value: usize,
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
pub struct GenerateEnergy {
    rate: usize,
}

pub struct DrainEnergy {
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
