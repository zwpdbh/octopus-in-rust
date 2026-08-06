#![allow(unused)]
use std::collections::HashMap;

use bevy_ecs::prelude::*;
use faf_blueprints::PlayerEcoMetrics;
use uuid::Uuid;

#[derive(Resource)]
pub struct PlayerEco(pub PlayerEcoMetrics);

impl PlayerEco {}
