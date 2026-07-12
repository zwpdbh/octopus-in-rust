//! Economy simulation plugin and data types.
//!
//! This module implements the FAF economy/build engine as a deterministic
//! Bevy ECS app. It exposes a plugin (`EcoPlugin`) that can be added to any
//! `App`, plus the input/output types used by [`crate::sim::Simulation`].

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use serde::{Deserialize, Serialize};

use crate::economy::{apply_tick_graph, EconomyState};
use crate::quantities::{Energy, EnergyRate, Mass, MassRate, Time};

/// Lightweight unit descriptor used by the simulator.
///
/// It deliberately does not depend on the full `Units` repository, so callers
/// (the Dioxus web app, the CLI, tests) can describe units with whatever data
/// they already have.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct UnitDefRef {
    /// Build power contributed while building. Zero for non-builders.
    #[serde(default)]
    pub build_power: f64,
    /// Mass required to build the unit.
    #[serde(default)]
    pub mass_cost: f64,
    /// Energy required to build the unit.
    #[serde(default)]
    pub energy_cost: f64,
    /// Build time at base build power (1.0).
    #[serde(default)]
    pub build_time: f64,
    /// Mass income per second after the unit is finished.
    #[serde(default)]
    pub mass_income: f64,
    /// Net energy income per second after the unit is finished (gross income
    /// minus maintenance, if any).
    #[serde(default)]
    pub energy_income: f64,
    /// Energy maintenance per second while the unit exists.
    #[serde(default)]
    pub maintenance_energy: f64,
    /// Mass storage capacity provided after the unit is finished.
    #[serde(default)]
    pub mass_storage: f64,
    /// Energy storage capacity provided after the unit is finished.
    #[serde(default)]
    pub energy_storage: f64,
}

impl UnitDefRef {
    /// Drain per second for building this unit with the given power.
    fn drain_per_second(&self, power: f64) -> (f64, f64) {
        if self.build_time <= 0.0 || power <= 0.0 {
            return (0.0, 0.0);
        }
        let progress_per_second = power / self.build_time;
        (
            progress_per_second * self.mass_cost,
            progress_per_second * self.energy_cost,
        )
    }
}

/// One task in a build queue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildTask {
    /// Caller-defined id, echoed back in start/complete events.
    pub id: u32,
    /// Simulation time at which the task may begin.
    pub start_after: Time,
    /// Builders assigned to the task.
    pub builders: Vec<UnitDefRef>,
    /// Unit being built.
    pub target: UnitDefRef,
}

/// A full build queue to simulate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildQueue {
    /// Initial economy state (income and storage).
    pub initial_eco: EconomyState,
    /// Tasks to run, in queue order.
    pub tasks: Vec<BuildTask>,
}

/// A point-in-time view of the economy.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EcoSnapshot {
    pub time: f64,
    pub mass_income: f64,
    pub energy_income: f64,
    pub total_mass_spent: f64,
    pub total_energy_spent: f64,
    pub mass_storage: f64,
    pub energy_storage: f64,
    pub mass_stalled: bool,
    pub energy_stalled: bool,
}

/// Observable event emitted by the simulation each step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SimulationEvent {
    /// A tick happened and the economy is in the given state.
    Ticked(EcoSnapshot),
    /// A task has become active.
    TaskStarted { task_id: u32, time: f64 },
    /// A task has finished.
    TaskCompleted { task_id: u32, time: f64 },
    /// The whole queue is done.
    Finished,
}

// ---------------------------------------------------------------------------
// ECS components and resources (crate-visible so sim.rs can drive them)
// ---------------------------------------------------------------------------

#[derive(Component)]
pub(crate) struct MaintenanceEnergy(f64);

#[derive(Component)]
pub(crate) struct Producer {
    pub(crate) mass_income: f64,
    pub(crate) energy_income: f64,
}

#[derive(Component)]
pub(crate) struct StorageContributor {
    pub(crate) mass: f64,
    pub(crate) energy: f64,
}

#[derive(Component)]
pub(crate) struct ConstructionSite {
    pub(crate) task_id: u32,
    pub(crate) target: UnitDefRef,
    pub(crate) remaining_work: f64,
    pub(crate) power: f64,
}

#[derive(Resource)]
pub(crate) struct SimClock {
    pub(crate) time: f64,
    pub(crate) dt: f64,
    pub(crate) max_time: Option<f64>,
}

#[derive(Resource)]
pub(crate) struct PendingTasks(pub(crate) Vec<BuildTask>);

#[derive(Resource)]
pub(crate) struct CompletedTasks(pub(crate) Vec<Entity>);

#[derive(Resource)]
pub(crate) struct EffectiveFactor(pub(crate) f64);

#[derive(Resource, Default)]
pub(crate) struct EventJournal(pub(crate) Vec<SimulationEvent>);

#[derive(Resource, Default)]
pub(crate) struct FinishedFlag(pub(crate) bool);

#[derive(Resource)]
pub(crate) struct EcoState(pub(crate) EconomyState);

#[derive(Resource)]
pub(crate) struct TotalsSpent {
    pub(crate) mass: f64,
    pub(crate) energy: f64,
}

/// Bevy plugin that registers the economy simulation systems.
///
/// Add this to an `App` to drive a build-queue simulation. Per-simulation data
/// (the queue, clock, etc.) is inserted by [`crate::sim::Simulation`].
pub struct EcoPlugin;

impl Plugin for EcoPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                spawn_tasks_system,
                recompute_base_economy_system,
                eco_system,
                progress_system,
                completion_system,
                termination_system,
            )
                .chain(),
        );
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

fn spawn_tasks_system(
    mut commands: Commands,
    mut pending: ResMut<PendingTasks>,
    clock: Res<SimClock>,
    mut journal: ResMut<EventJournal>,
) {
    let now = clock.time;
    let mut started = Vec::new();

    pending.0.retain(|task| {
        if task.start_after.value() <= now {
            let total_power: f64 = task.builders.iter().map(|b| b.build_power).sum();
            let total_work = task.target.build_time.max(0.0);

            for builder in &task.builders {
                commands.spawn((MaintenanceEnergy(builder.maintenance_energy),));
            }

            commands.spawn((ConstructionSite {
                task_id: task.id,
                target: task.target,
                remaining_work: total_work,
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
            time: now,
        });
    }
}

fn recompute_base_economy_system(
    producers: Query<&Producer>,
    maintenance: Query<&MaintenanceEnergy>,
    storage: Query<&StorageContributor>,
    mut eco: ResMut<EcoState>,
) {
    let eco = &mut eco.0;
    let mut mass_income = 0.0;
    let mut energy_income = 0.0;
    for p in producers.iter() {
        mass_income += p.mass_income;
        energy_income += p.energy_income;
    }
    for m in maintenance.iter() {
        energy_income -= m.0;
    }

    let mut mass_cap = 0.0;
    let mut energy_cap = 0.0;
    for s in storage.iter() {
        mass_cap += s.mass;
        energy_cap += s.energy;
    }

    eco.net_mass_income = MassRate::from_raw(mass_income);
    eco.net_energy_income = EnergyRate::from_raw(energy_income);
    eco.mass_storage.cap = Mass::from_raw(mass_cap.max(0.0));
    eco.energy_storage.cap = Energy::from_raw(energy_cap.max(0.0));
}

fn eco_system(
    sites: Query<&ConstructionSite>,
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
        let (mass, energy) = site.target.drain_per_second(site.power);
        total_mass_drain += mass;
        total_energy_drain += energy;
    }

    let result = apply_tick_graph(total_mass_drain, total_energy_drain, &eco.0, dt);

    let eco = &mut eco.0;

    eco.mass_storage.current = result.new_mass_storage.current;
    eco.mass_storage.cap = result.new_mass_storage.cap;
    eco.energy_storage = result.new_energy_storage;
    eco.net_mass_income = result.scaled_net_mass_income;
    factor.0 = result.effective_factor;

    totals.mass += result.mass_consumed.value();
    totals.energy += result.energy_consumed.value();

    clock.time += dt;

    journal.0.push(SimulationEvent::Ticked(EcoSnapshot {
        time: clock.time,
        mass_income: result.scaled_net_mass_income.value(),
        energy_income: eco.net_energy_income.value(),
        total_mass_spent: totals.mass,
        total_energy_spent: totals.energy,
        mass_storage: eco.mass_storage.current.value(),
        energy_storage: eco.energy_storage.current.value(),
        mass_stalled: result.mass_stalled,
        energy_stalled: result.energy_stalled,
    }));
}

fn progress_system(
    mut sites: Query<(Entity, &mut ConstructionSite)>,
    factor: Res<EffectiveFactor>,
    clock: Res<SimClock>,
    mut completed: ResMut<CompletedTasks>,
) {
    let dt = clock.dt;
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

fn completion_system(
    mut commands: Commands,
    mut completed: ResMut<CompletedTasks>,
    sites: Query<&ConstructionSite>,
    clock: Res<SimClock>,
    mut journal: ResMut<EventJournal>,
) {
    let now = clock.time;
    let mut finished_ids = Vec::new();

    for entity in completed.0.drain(..) {
        let Ok(site) = sites.get(entity) else {
            continue;
        };

        finished_ids.push(site.task_id);
        commands.entity(entity).despawn();

        let target = site.target;
        commands.spawn((
            Producer {
                mass_income: target.mass_income,
                energy_income: target.energy_income - target.maintenance_energy,
            },
            StorageContributor {
                mass: target.mass_storage,
                energy: target.energy_storage,
            },
        ));
    }

    for id in finished_ids {
        journal.0.push(SimulationEvent::TaskCompleted {
            task_id: id,
            time: now,
        });
    }
}

fn termination_system(
    pending: Res<PendingTasks>,
    sites: Query<&ConstructionSite>,
    clock: Res<SimClock>,
    mut finished: ResMut<FinishedFlag>,
    mut journal: ResMut<EventJournal>,
) {
    if finished.0 {
        return;
    }

    let timed_out = clock.max_time.map_or(false, |max| clock.time >= max);
    let queue_empty = pending.0.is_empty() && sites.is_empty();

    if timed_out || queue_empty {
        finished.0 = true;
        journal.0.push(SimulationEvent::Finished);
    }
}
