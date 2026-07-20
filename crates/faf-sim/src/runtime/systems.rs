//! Bevy ECS systems that drive the economy simulation.
//!
//! Systems run in this order each update (see [`crate::runtime::BuildQueueSimulationPlugin`]):
//!
//! 1. `spawn_tasks_system` — activate tasks whose `ready_at` time has arrived.
//! 2. `recompute_base_economy_system` — aggregate producer income, maintenance,
//!    and storage capacity.
//! 3. `eco_system` — run the global economy tick, update storage, emit snapshots.
//! 4. `progress_system` — advance construction sites using the global stall factor.
//! 5. `completion_system` — finish targets, spawn producers, unlock next tasks.
//! 6. `termination_system` — detect when the queue is done or timed out.
//!
//! A startup system, `seed_initial_economy_system`, creates the initial economy
//! entities from the `EcoState` resource inserted by [`crate::sim::Simulation`].

use bevy_ecs::prelude::*;

use crate::economy::apply_tick_graph;
use crate::quantities::{Energy, EnergyRate, Mass, MassRate, Time};
use crate::runtime::components::{
    ActiveBuildTask, AdjacencyBonusComp, Producer, StorageContributor,
};
use crate::runtime::resources::{
    CompletedTasks, EcoState, EffectiveFactor, EventJournal, FinishedFlag, PendingTasks,
    PostQueueTailSeconds, SimClock, TailEndTime, TotalsSpent,
};
use crate::runtime::types::{EcoSnapshot, SimulationEvent};

/// Spawn the initial economy entities from the [`EcoState`] resource.
///
/// This runs once at startup before the first update, so the entities exist
/// when `recompute_base_economy_system` first aggregates income and storage.
pub(crate) fn seed_initial_economy_system(mut commands: Commands, eco: Res<EcoState>) {
    commands.spawn((
        Producer {
            production_per_second_mass: eco.0.production_per_second_mass.value(),
            production_per_second_energy: eco.0.production_per_second_energy.value(),
            maintenance_consumption_per_second_energy: eco
                .0
                .maintenance_consumption_per_second_energy
                .value(),
        },
        AdjacencyBonusComp::default(),
    ));
    commands.spawn(StorageContributor {
        mass: eco.0.mass_storage.cap.value(),
        energy: eco.0.energy_storage.cap.value(),
    });
}

pub(crate) fn spawn_tasks_system(
    mut commands: Commands,
    mut pending: ResMut<PendingTasks>,
    clock: Res<SimClock>,
    mut journal: ResMut<EventJournal>,
) {
    let now = clock.time;
    let mut started = Vec::new();

    pending.0.retain(|scheduled| {
        if scheduled.ready_at <= now {
            let task = &scheduled.task;
            let total_power: f64 = task.builders.iter().map(|b| b.build_power).sum();
            let first_work = task
                .targets
                .first()
                .map(|t| t.build_time.max(0.0))
                .unwrap_or(0.0);

            for builder in &task.builders {
                commands.spawn((
                    Producer {
                        production_per_second_mass: 0.0,
                        production_per_second_energy: 0.0,
                        maintenance_consumption_per_second_energy: builder
                            .maintenance_consumption_per_second_energy,
                    },
                    AdjacencyBonusComp::default(),
                ));
            }

            commands.spawn((ActiveBuildTask {
                task_id: task.id,
                targets: task.targets.clone(),
                current_target_index: 0,
                remaining_work: first_work,
                power: total_power,
            },));

            started.push(task.id);
            false
        } else {
            true
        }
    });

    for id in started {
        journal.0.push(SimulationEvent::TaskStarted {
            task_id: id,
            time: now.value(),
        });
    }
}

pub(crate) fn recompute_base_economy_system(
    producers: Query<(&Producer, &AdjacencyBonusComp)>,
    storage: Query<&StorageContributor>,
    mut eco: ResMut<EcoState>,
) {
    let eco = &mut eco.0;
    let mut production_per_second_mass = 0.0;
    let mut production_per_second_energy = 0.0;
    let mut maintenance_consumption_per_second_energy = 0.0;
    for (p, bonus) in producers.iter() {
        production_per_second_mass +=
            p.production_per_second_mass * bonus.0.mass_production_multiplier();
        production_per_second_energy +=
            p.production_per_second_energy * bonus.0.energy_production_multiplier();
        maintenance_consumption_per_second_energy += p.maintenance_consumption_per_second_energy;
    }

    let mut mass_cap = 0.0;
    let mut energy_cap = 0.0;
    for s in storage.iter() {
        mass_cap += s.mass;
        energy_cap += s.energy;
    }

    eco.production_per_second_mass = MassRate::from_raw(production_per_second_mass);
    eco.production_per_second_energy = EnergyRate::from_raw(production_per_second_energy);
    eco.maintenance_consumption_per_second_energy =
        EnergyRate::from_raw(maintenance_consumption_per_second_energy);

    eco.mass_storage.cap = Mass::from_raw(mass_cap.max(0.0));
    eco.energy_storage.cap = Energy::from_raw(energy_cap.max(0.0));
}

pub(crate) fn eco_system(
    sites: Query<&ActiveBuildTask>,
    mut eco: ResMut<EcoState>,
    mut clock: ResMut<SimClock>,
    mut factor: ResMut<EffectiveFactor>,
    mut journal: ResMut<EventJournal>,
    mut totals: ResMut<TotalsSpent>,
) {
    if let Some(max_time) = clock.max_time {
        if clock.time >= max_time {
            return;
        }
    }

    let dt = clock.dt;

    let mut total_mass_drain = 0.0;
    let mut total_energy_drain = 0.0;
    for site in sites.iter() {
        if site.power <= 0.0 {
            continue;
        }
        if let Some(target) = site.targets.get(site.current_target_index) {
            let (mass, energy) = target.drain_per_second(site.power);
            total_mass_drain += mass;
            total_energy_drain += energy;
        }
    }

    let result = apply_tick_graph(total_mass_drain, total_energy_drain, &eco.0, dt.value());

    let eco = &mut eco.0;

    eco.mass_storage.current = result.new_mass_storage.current;
    eco.mass_storage.cap = result.new_mass_storage.cap;
    eco.energy_storage = result.new_energy_storage;

    factor.0 = result.effective_factor;

    totals.mass += result.mass_consumed.value();
    totals.energy += result.energy_consumed.value();

    clock.time = clock.time + dt;

    journal.0.push(SimulationEvent::Ticked(EcoSnapshot {
        time: clock.time.value(),
        production_per_second_mass: eco.production_per_second_mass.value(),
        production_per_second_energy: eco.production_per_second_energy.value(),
        maintenance_consumption_per_second_energy: eco
            .maintenance_consumption_per_second_energy
            .value(),
        mass_drain: total_mass_drain,
        energy_drain: total_energy_drain,
        total_mass_spent: totals.mass,
        total_energy_spent: totals.energy,
        mass_storage: eco.mass_storage.current.value(),
        mass_storage_cap: eco.mass_storage.cap.value(),
        energy_storage: eco.energy_storage.current.value(),
        energy_storage_cap: eco.energy_storage.cap.value(),
    }));
}

pub(crate) fn progress_system(
    mut sites: Query<(Entity, &mut ActiveBuildTask)>,
    factor: Res<EffectiveFactor>,
    clock: Res<SimClock>,
    mut completed: ResMut<CompletedTasks>,
) {
    let dt = clock.dt.value();
    let effective = factor.0;

    for (entity, mut site) in sites.iter_mut() {
        if site.power <= 0.0 || site.remaining_work <= 0.0 {
            continue;
        }
        let progress = effective * site.power * dt;
        if progress > 0.0 && site.remaining_work <= progress {
            completed.0.push(entity);
        } else {
            site.remaining_work -= progress;
        }
    }
}

pub(crate) fn completion_system(
    mut commands: Commands,
    mut completed: ResMut<CompletedTasks>,
    sites: Query<&ActiveBuildTask>,
    clock: Res<SimClock>,
    mut journal: ResMut<EventJournal>,
    mut pending: ResMut<PendingTasks>,
) {
    let now = clock.time.value();
    let mut finished_ids = Vec::new();

    for entity in completed.0.drain(..) {
        let Ok(site) = sites.get(entity) else {
            continue;
        };

        let Some(target) = site.targets.get(site.current_target_index) else {
            continue;
        };

        // The current target is finished; it starts contributing economy
        // immediately if it has production/storage stats.
        commands.spawn((
            Producer {
                production_per_second_mass: target.production_per_second_mass,
                production_per_second_energy: target.production_per_second_energy,
                maintenance_consumption_per_second_energy: target
                    .maintenance_consumption_per_second_energy,
            },
            StorageContributor {
                mass: target.mass_storage,
                energy: target.energy_storage,
            },
            AdjacencyBonusComp(target.adjacency),
        ));

        let next_index = site.current_target_index + 1;
        if next_index < site.targets.len() {
            // Move on to the next target in the same task.
            let next_work = site.targets[next_index].build_time.max(0.0);
            commands.entity(entity).insert(ActiveBuildTask {
                task_id: site.task_id,
                targets: site.targets.clone(),
                current_target_index: next_index,
                remaining_work: next_work,
                power: site.power,
            });
        } else {
            // All targets in the task are done.
            finished_ids.push(site.task_id);
            commands.entity(entity).despawn();
        }
    }

    // A finished task unlocks the next pending task after its `start_after`
    // delay relative to this finish time.
    if !finished_ids.is_empty() {
        if let Some(next) = pending.0.first_mut() {
            next.ready_at = clock.time + next.task.start_after;
        }
    }

    for id in finished_ids {
        journal.0.push(SimulationEvent::TaskCompleted {
            task_id: id,
            time: now,
        });
    }
}

pub(crate) fn termination_system(
    pending: Res<PendingTasks>,
    sites: Query<&ActiveBuildTask>,
    clock: Res<SimClock>,
    mut finished: ResMut<FinishedFlag>,
    mut journal: ResMut<EventJournal>,
    mut tail_end: ResMut<TailEndTime>,
    tail_seconds: Res<PostQueueTailSeconds>,
) {
    if finished.0 {
        return;
    }

    let timed_out = clock.max_time.is_some_and(|max| clock.time >= max);
    let queue_empty = pending.0.is_empty() && sites.is_empty();

    if timed_out {
        finished.0 = true;
        journal.0.push(SimulationEvent::Finished);
        return;
    }

    if queue_empty {
        match (tail_seconds.0, tail_end.0) {
            (None, _) => {
                finished.0 = true;
                journal.0.push(SimulationEvent::Finished);
            }
            (Some(_), Some(end)) if clock.time >= end => {
                finished.0 = true;
                journal.0.push(SimulationEvent::Finished);
            }
            (Some(seconds), None) => {
                tail_end.0 = Some(clock.time + Time::from_raw(seconds));
            }
            _ => {}
        }
    }
}
