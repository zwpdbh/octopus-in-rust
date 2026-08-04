#![allow(unused)]
use bevy_ecs::prelude::*;
use uuid::Uuid;

use crate::components::*;

#[derive(Event)]
pub struct BuildingFinished {
    pub task_id: Uuid,
}

pub fn clean_up_finished_tasks(
    finished_task: On<BuildingFinished>,
    mut commands: Commands,
    construction_builder_query: Query<(Entity, &ConstructionBuilder), (With<ConstructionBuilder>)>,
    construction_target_query: Query<(Entity, &ConstructionTarget), (With<ConstructionTarget>)>,
) {
    for (each_builder_entity, builder) in construction_builder_query {
        if builder.task == finished_task.task_id {
            commands
                .entity(each_builder_entity)
                .remove::<ConstructionBuilder>();
        }
    }

    for (each_target_entity, target) in construction_target_query {
        if target.task == finished_task.task_id {
            commands
                .entity(each_target_entity)
                .remove::<ConstructionTarget>();
        }
    }
}

#[derive(Event)]
pub struct PlayerEcoSummary {
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
