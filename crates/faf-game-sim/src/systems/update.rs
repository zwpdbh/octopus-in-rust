#![allow(unused)]
use std::collections::HashMap;
use std::collections::HashSet;

use crate::components::*;
use crate::events::*;
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
    construction_builder_query: Query<
        (Entity, &BuildPower, &ConstructionBuilder),
        (With<BuildPower>, With<ConstructionBuilder>),
    >,
    construction_target_query: Query<
        (Entity, &UnitCost, &ConstructionTarget),
        (With<UnitCost>, With<ConstructionTarget>),
    >,
) {
    let mut build_powers_tracking: HashMap<Uuid, f64> = HashMap::new();

    for (_, build_power, builder) in construction_builder_query {
        build_powers_tracking
            .entry(builder.task)
            .and_modify(|bp| *bp += build_power.0)
            .or_insert(build_power.0);
    }

    let mut build_cost_tracking: HashMap<Uuid, UnitCost> = HashMap::new();
    for (_, unit_cost, target) in construction_target_query {
        build_cost_tracking
            .entry(target.task)
            .insert_entry(*unit_cost);
    }

    // based on each target, compute the drain
    for (task, build_cost) in build_cost_tracking {
        let assigned_build_power = build_powers_tracking.get(&task).unwrap();
        let drain_ratio = build_cost.build_time / assigned_build_power;
        let mass_drain = build_cost.mass / drain_ratio;
        let energy_drain = build_cost.energy / drain_ratio;

        player_eco.mass_drain += mass_drain;
        player_eco.energy_drain += energy_drain;
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

type AccumulatedBP = f64;
type AccumulatedBuildTime = f64;

// step 4.2: update each construction progress basedon efficiency ratio
pub fn update_construction_pragress(
    mut commands: Commands,
    player_eco: Res<PlayerEco>,
    construction_builder_query: Query<
        (Entity, &BuildPower, &ConstructionBuilder),
        (With<BuildPower>, With<ConstructionBuilder>),
    >,
    construction_target_query: Query<
        (Entity, &UnitCost, &ConstructionTarget),
        (With<UnitCost>, With<ConstructionTarget>),
    >,
    // mut ev_building_finished: EventWriter<BuildingFinished>,
) {
    let mut build_powers_tracking: HashMap<Uuid, f64> = HashMap::new();
    for (_, build_power, builder) in construction_builder_query {
        build_powers_tracking
            .entry(builder.task)
            .and_modify(|bp| *bp += build_power.0)
            .or_insert(build_power.0);
    }

    let mut build_progress_tracking: HashMap<Uuid, (f64, f64)> = HashMap::new();
    for (entity, unit_cost, target) in construction_target_query {
        let task_id = target.task;
        let current_progress = target.progress;

        let assigned_bp_for_task = build_powers_tracking.get(&task_id).unwrap();
        // how to set components value
        commands.entity(entity).insert(ConstructionTarget::new(
            task_id,
            current_progress + assigned_bp_for_task,
        ));

        if current_progress + assigned_bp_for_task > unit_cost.build_time {
            commands.trigger(BuildingFinished { task_id });
        }
    }
}
