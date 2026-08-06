#![allow(unused)]
use bevy_ecs::prelude::*;
use faf_blueprints::PlayerEcoMetrics;
use uuid::Uuid;

#[derive(Message)]
pub struct BuildingFinished {
    pub task_id: Uuid,
}

#[derive(Event)]
pub struct PlayerEcoSummary(pub PlayerEcoMetrics);

fn print_eco_summary(summary: On<PlayerEcoSummary>, mut _commands: Commands) {
    println!("eco summary => {:?}", summary.0);
}
