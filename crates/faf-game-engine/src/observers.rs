#![allow(unused)]
use bevy_ecs::prelude::*;
use faf_blueprints::PlayerEcoMetrics;
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
pub struct PlayerEcoSummary(pub PlayerEcoMetrics);
