#![allow(unused)]
use bevy_ecs::prelude::*;
use uuid::Uuid;

#[derive(Event)]
pub struct BuildingFinished {
    pub task_id: Uuid,
}
