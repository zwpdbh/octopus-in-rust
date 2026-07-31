#![allow(unused)]

use bevy_ecs::prelude::*;
use uuid::Uuid;
#[derive(Component)]
pub struct Unit {
    id: Uuid,
    name: String,
    nick_name: Option<String>,
}

#[derive(Component, Copy, Clone)]
pub struct UnitCost {
    pub mass: f64,
    pub energy: f64,
    pub build_time: f64,
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
    pub rate: f64,
}

#[derive(Component)]
pub struct GenerateEnergy {
    pub rate: f64,
}

#[derive(Component)]
pub struct DrainMaintainanceEnergy {
    pub rate: f64,
}

#[derive(Component)]
pub struct IncreaseMassStorage {
    pub capacity: f64,
}

#[derive(Component)]
pub struct IncreaseEnergyStorage {
    pub capacity: f64,
}

#[derive(Component)]
pub enum BuildingInProgress {
    Target { task: Uuid },
    Builder { task: Uuid, build_power: f64 },
}
