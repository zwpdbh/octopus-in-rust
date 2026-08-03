#![allow(unused)]
use std::collections::HashMap;
use std::collections::HashSet;

use crate::components::*;
use crate::resources::*;
use bevy_ecs::prelude::*;
use uuid::Uuid;

// step 1. compute current static eco production and drain from existing buildings
pub fn update_player_eco_from_existing_unit(
    mut player_eco: ResMut<PlayerEco>,
    query_eco_related_units: Query<
        (
            Entity,
            &GenerateMass,
            &GenerateEnergy,
            &MaintainancePowerDrain,
        ),
        (
            With<GenerateMass>,
            With<GenerateEnergy>,
            With<MaintainancePowerDrain>,
        ),
    >,
) {
    // reset to zero first, then aggregate
    player_eco.mass_generate_rate = 0.0;
    player_eco.energy_generate_rate = 0.0;
    player_eco.energy_drain = 0.0;

    for (_, generate_mass, generate_energy, maintainance_energy_drain) in query_eco_related_units {
        player_eco.mass_generate_rate += generate_mass.0;
        player_eco.energy_generate_rate += generate_energy.0;
        player_eco.energy_generate_rate += maintainance_energy_drain.0;
    }
}

// step 2.
// A system which aggregate all mass drain and energy drain from all building tasks
pub fn update_player_eco_from_construction(
    mut player_eco: ResMut<PlayerEco>,
    mut constructions: ResMut<Constructions>,
    construction_role_query: Query<(Entity, &UnitCost, &ConstructionRole), With<ConstructionRole>>,
) {
    // Aggregate mass drain and energy drain from construction on some task.
    // key is task id
    // value is (unit_cost, assigned_bp)
    let mut building_records: HashMap<Uuid, (UnitCost, f64)> = HashMap::new();

    for (_each_entity, unit_cost, role) in construction_role_query {
        match role {
            ConstructionRole::Target { task, .. } => {
                let _ = building_records
                    .entry(*task)
                    .and_modify(|(unit_cost, _build_power)| *unit_cost = *unit_cost)
                    .or_insert((*unit_cost, 0.0));
            }
            ConstructionRole::Builder {
                task,
                build_power: builder_bp,
            } => {
                let _ = building_records
                    .entry(*task)
                    .and_modify(|(_unit_cost, build_power)| *build_power += *builder_bp)
                    .or_insert((*unit_cost, *builder_bp));
            }
        }
    }

    for (task, (unit_cost, build_power)) in building_records {
        let build_ratio = unit_cost.build_time / build_power;
        let mass_drain = unit_cost.mass / build_ratio;
        let energy_drain = unit_cost.energy / build_ratio;

        player_eco.mass_drain += mass_drain;
        player_eco.energy_drain += energy_drain;

        // remember to initalize construction record when spawn component for ConstructionRole
        constructions.records.entry(task).and_modify(
            |(
                current_mass_drain,
                current_energy_drain,
                _target_build_time,
                current_bp_assigned,
                _current_progress,
            )| {
                *current_mass_drain = mass_drain;
                *current_energy_drain = energy_drain;
                *current_bp_assigned = build_power;
            },
        );
    }
}

// step3, update storage
// emit stall or overflow event
pub fn update_player_eco_storage_metrics(mut player_eco: ResMut<PlayerEco>) {
    let net_mass_rate = player_eco.net_mass_rate();
    let net_energy_rate = player_eco.net_energy_rate();

    let udpated_mass_in_storage = if player_eco.mass_in_storage + net_mass_rate > 0.0 {
        player_eco
            .max_capacity_in_mass_storage
            .min(player_eco.mass_in_storage + net_mass_rate)
    } else {
        0.0
    };

    let updated_energy_in_storage = if player_eco.energy_in_storage + net_energy_rate > 0.0 {
        player_eco
            .max_capacity_in_energy_storage
            .min(player_eco.energy_in_storage + net_energy_rate)
    } else {
        0.0
    };

    player_eco.mass_in_storage = udpated_mass_in_storage;
    player_eco.energy_in_storage = updated_energy_in_storage;
}

// state 4.1: update mass_production from efficiency
pub fn check_player_eco_from_power_stall(mut player: ResMut<PlayerEco>) {
    if player.energy_efficiency() < 1.0 {
        player.mass_generate_rate = player.mass_generate_rate * player.energy_efficiency();
    }
}

// step 4.2: update each construction progress basedon efficiency ratio
pub fn update_construction_pragress(
    mut commands: Commands,
    player_eco: Res<PlayerEco>,
    mut constructions: ResMut<Constructions>,
    constructions_query: Query<(Entity, &ConstructionRole), With<ConstructionRole>>,
) {
    let construction_efficiency = player_eco.construction_efficiency();

    let mut finished_constructions: HashSet<Uuid> = HashSet::new();

    for (task_id, (mass_drain, energy_drain, build_time, bp, construction_progress)) in
        constructions.records.clone()
    {
        let effective_bp = bp * construction_efficiency;

        constructions
            .records
            .entry(task_id)
            .and_modify(|(_, _, _, _, current_progress)| *current_progress += effective_bp)
            .or_insert((
                mass_drain,
                energy_drain,
                build_time,
                bp,
                construction_progress,
            ));

        let (_mass_drain, _energy_drain, build_time, _bp, construction_progress) =
            constructions.records.get(&task_id).unwrap();
        if construction_progress >= build_time {
            finished_constructions.insert(task_id);
        }
    }

    // check each related entity to see if it participate the construction which is finished
    for (related_entity, role) in constructions_query {
        match role {
            ConstructionRole::Builder { task, .. } => {
                if finished_constructions.contains(task) {
                    commands.entity(related_entity).remove::<ConstructionRole>();
                }
            }
            ConstructionRole::Target { task, eco_building } => {
                if finished_constructions.contains(task) {
                    let eco_building_bundle = EcoBuilding::new(
                        eco_building.generate_mass.0,
                        eco_building.generate_energy.0,
                        eco_building.maintainance_power_drain.0,
                    );
                    commands
                        .entity(related_entity)
                        .insert(eco_building_bundle)
                        .remove::<ConstructionRole>();
                }
            }
        }
    }

    // also destory construction record
    for each in finished_constructions {
        constructions.records.remove(&each);
        // IMPROVE:: notice one construction has finished
    }
}
