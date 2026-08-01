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
pub struct EcoBuilding {
    pub generate_mass: Option<f64>,
    pub generate_energy: Option<f64>,
    pub maintainance_power_drain: Option<f64>,
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
pub enum ConstructionRole {
    Target { task: Uuid },
    Builder { task: Uuid, build_power: f64 },
}
