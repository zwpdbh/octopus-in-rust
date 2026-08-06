#![allow(unused)]

use bevy_ecs::prelude::*;
use faf_blueprints::*;
use uuid::Uuid;

#[derive(Component, Copy, Clone)]
pub struct UnitCost(pub UnitCostMetrics);

impl UnitCost {}

#[derive(Component)]
pub struct UnitTechLevel(pub TechLevel);

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
    pub unit_eco_effect: UnitEffectEcoMetrics,
    pub tech_level: TechLevel,
}

impl ConstructionTarget {
    pub fn new(
        task: Uuid,
        progress: f64,
        unit_eco_effect: UnitEffectEcoMetrics,
        tech_level: TechLevel,
    ) -> ConstructionTarget {
        Self {
            task,
            progress,
            unit_eco_effect,
            tech_level,
        }
    }
}
