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

impl UnitCost {
    pub fn default() -> Self {
        Self {
            mass: 0.0,
            energy: 0.0,
            build_time: 0.0,
        }
    }
}

#[derive(Component)]
pub enum UnitTechLevel {
    T1,
    T2,
    T3,
    T4,
}

#[derive(Bundle)]
pub struct EcoEffectMetrics {
    pub generate_mass: GenerateMass,
    pub generate_energy: GenerateEnergy,
    pub maintainance_power_drain: MaintainancePowerDrain,
    pub build_power: BuildPower,
}

impl EcoEffectMetrics {
    pub fn new(
        generate_mass: f64,
        generate_energy: f64,
        maintainance_energy_drain: f64,
        build_power: f64,
    ) -> Self {
        Self {
            generate_mass: GenerateMass(generate_mass),
            generate_energy: GenerateEnergy(generate_energy),
            maintainance_power_drain: MaintainancePowerDrain(maintainance_energy_drain),
            build_power: BuildPower(build_power),
        }
    }
}

#[derive(Component)]
pub struct BuildPower(pub f64);

#[derive(Component)]
pub struct GenerateMass(pub f64);

#[derive(Component)]
pub struct GenerateEnergy(pub f64);

#[derive(Component)]
pub struct MaintainancePowerDrain(pub f64);

#[derive(Component)]
pub struct IncreaseMassStorage {
    pub capacity: f64,
}

#[derive(Component)]
pub struct IncreaseEnergyStorage {
    pub capacity: f64,
}

#[derive(Component)]
pub struct ConstructionBuilder {
    pub task: Uuid,
}

#[derive(Component)]
pub struct ConstructionTarget {
    pub task: Uuid,
    pub progress: f64,
}

impl ConstructionTarget {
    pub fn new(task: Uuid, progress: f64) -> Self {
        Self { task, progress }
    }
}
