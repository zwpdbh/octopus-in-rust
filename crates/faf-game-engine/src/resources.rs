use bevy_ecs::prelude::*;
use faf_blueprints::PlayerEcoMetrics;

#[derive(Resource, Default)]
pub struct PlayerEco(pub PlayerEcoMetrics);

impl PlayerEco {}
