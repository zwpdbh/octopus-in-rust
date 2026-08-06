#![allow(unused)]

use bevy_ecs::prelude::*;
use faf_blueprints::*;
use uuid::Uuid;

#[derive(Component)]
pub struct Unit {
    id: Uuid,
    name: String,
    nick_name: Option<String>,
}

#[derive(Component, Copy, Clone)]
pub struct UnitCost(pub UnitCostMetrics);

impl UnitCost {}

#[derive(Component)]
pub struct UnitTechLevel(pub TechLevel);

#[derive(Bundle)]
pub struct EcoEffectMetrics {
    pub generate_mass: GenerateMass,
    pub generate_energy: GenerateEnergy,
    pub maintainance_power_drain: MaintainancePowerDrain,
    pub increase_mass_storage_capacity: IncreaseMassStorageCapacity,
    pub increase_energy_storage_capacity: IncreaseEnergyStorageCapacity,
    pub build_power: BuildPower,
}

impl EcoEffectMetrics {
    pub fn new(
        generate_mass: f64,
        generate_energy: f64,
        maintainance_energy_drain: f64,
        increase_mass_storage_capacity: f64,
        increase_energy_storage_capacity: f64,
        build_power: f64,
    ) -> EcoEffectMetrics {
        Self {
            generate_mass: GenerateMass(generate_mass),
            generate_energy: GenerateEnergy(generate_energy),
            increase_mass_storage_capacity: IncreaseMassStorageCapacity(
                increase_mass_storage_capacity,
            ),
            increase_energy_storage_capacity: IncreaseEnergyStorageCapacity(
                increase_energy_storage_capacity,
            ),
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
pub struct IncreaseMassStorageCapacity(pub f64);

#[derive(Component)]
pub struct IncreaseEnergyStorageCapacity(pub f64);

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
