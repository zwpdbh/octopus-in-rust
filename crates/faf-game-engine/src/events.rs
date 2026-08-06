use bevy_ecs::prelude::*;
use faf_blueprints::PlayerEcoMetrics;
use uuid::Uuid;

#[derive(Message)]
pub struct BuildingFinished {
    pub task_id: Uuid,
}

#[allow(unused)]
#[derive(Event)]
pub struct PlayerEcoSummary(pub PlayerEcoMetrics);
