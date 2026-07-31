#![allow(unused)]
use std::collections::HashMap;

use super::components::*;
use super::resources::*;
use bevy_ecs::prelude::*;
use uuid::Uuid;

// step 1. compute current static eco production and drain from existing buildings
fn update_player_eco_from_existing_unit(
    mut player_eco: ResMut<PlayerEco>,
    building_generate_mass: Query<(Entity, Option<&GenerateMass>)>,
    building_generate_energy: Query<(Entity, Option<&GenerateEnergy>)>,
    building_drain_energy: Query<(Entity, Option<&DrainMaintainanceEnergy>)>,
) {
    // reset to zero first, then aggregate
    player_eco.mass_generate_rate = 0.0;
    player_eco.energy_generate_rate = 0.0;
    player_eco.energy_drain = 0.0;

    for (_, generate_mass) in building_generate_mass {
        if let Some(GenerateMass { rate }) = generate_mass {
            player_eco.mass_generate_rate += rate
        }
    }

    for (_, generate_energy) in building_generate_energy {
        if let Some(GenerateEnergy { rate }) = generate_energy {
            player_eco.energy_generate_rate += rate
        }
    }

    for (_, buidling_drain) in building_drain_energy {
        if let Some(DrainMaintainanceEnergy { rate }) = buidling_drain {
            player_eco.energy_drain += rate
        }
    }
}

// step 2.
// A system which aggregate all mass drain and energy drain from all building tasks
fn update_player_eco_from_construction(
    mut player_eco: ResMut<PlayerEco>,
    building_query: Query<(Entity, &UnitCost, Option<&BuildingInProgress>)>,
) {
    let mut building_records: HashMap<Uuid, (UnitCost, f64)> = HashMap::new();

    for (_each_entity, unit_cost, building_in_progress) in building_query {
        if let Some(building_in_progress) = building_in_progress {
            match building_in_progress {
                BuildingInProgress::Target { task } => {
                    let _ = building_records
                        .entry(*task)
                        .and_modify(|(unit_cost, _build_power)| *unit_cost = *unit_cost);
                }
                BuildingInProgress::Builder { task, build_power } => {
                    let _ = building_records
                        .entry(*task)
                        .and_modify(|(_unit_cost, build_power)| *build_power += *build_power);
                }
            }
        }
    }

    for (_, (unit_cost, build_power)) in building_records {
        let build_ratio = unit_cost.build_time / build_power;
        let mass_drain = unit_cost.mass / build_ratio;
        let energy_drain = unit_cost.energy / build_ratio;

        player_eco.mass_drain += mass_drain;
        player_eco.energy_drain += energy_drain;
    }
}

// step3, update storage
// emit stall or overflow event
fn update_player_eco_for_storage(mut player_eco: ResMut<PlayerEco>) {
    player_eco.update_storage();
    // TODO:: emit storage overflow or mass stall or energy stall event
}
