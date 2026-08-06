use bevy_ecs::prelude::*;
use faf_blueprints::PlayerEcoMetrics;

#[derive(Resource)]
pub struct PlayerEco(pub PlayerEcoMetrics);

impl PlayerEco {}
