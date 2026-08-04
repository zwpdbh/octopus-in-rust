#![allow(unused)]
use bevy::{ecs::schedule::*, prelude::*};

use crate::resources::PlayerEco;

struct EcoPlugin;

impl Plugin for EcoPlugin {
    fn build(&self, app: &mut App) {
        let player_eco = PlayerEco::default();
        app.insert_resource(player_eco);
    }
}
