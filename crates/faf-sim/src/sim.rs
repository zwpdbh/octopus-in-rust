//! Simulation entry point.
//!
//! This module provides [`Simulation`], the high-level synchronous driver that
//! owns a Bevy `App`, wires in the [`EcoPlugin`](crate::eco::EcoPlugin), and
//! lets callers step the simulation one tick at a time.
//!
//! The input/output types are defined in [`crate::eco`] and re-exported here
//! so consumers have a single obvious import path.

use bevy_app::prelude::*;

pub use crate::eco::{BuildQueue, BuildTask, EcoPlugin, EcoSnapshot, SimulationEvent, UnitDefRef};
use crate::eco::{
    CompletedTasks, EcoState, EffectiveFactor, EventJournal, FinishedFlag, PendingTasks, Producer,
    SimClock, StorageContributor, TotalsSpent,
};

/// Steppable economy simulation.
pub struct Simulation {
    app: App,
}

impl Simulation {
    /// Create a new simulation for the given queue.
    ///
    /// `dt` is the simulation step size in seconds. `max_time` is an optional
    /// hard cap in seconds to prevent infinite simulation; when `None` the
    /// simulation runs until the build queue is empty.
    pub fn new(queue: BuildQueue, dt: f64, max_time: Option<f64>) -> Self {
        let mut app = App::new();
        app.add_plugins(EcoPlugin)
            .insert_resource(SimClock {
                time: 0.0,
                dt,
                max_time,
            })
            .insert_resource(PendingTasks(queue.tasks))
            .insert_resource(CompletedTasks(Vec::new()))
            .insert_resource(EffectiveFactor(1.0))
            .insert_resource(EventJournal::default())
            .insert_resource(FinishedFlag::default())
            .insert_resource(TotalsSpent {
                mass: 0.0,
                energy: 0.0,
            });

        // Seed the world with the initial economy so recompute_base_economy_system
        // preserves the caller's starting income and storage capacity.
        {
            let initial = queue.initial_eco;
            let world = app.world_mut();
            world.spawn((Producer {
                mass_income: initial.net_mass_income.value(),
                energy_income: initial.net_energy_income.value(),
            },));
            world.spawn((StorageContributor {
                mass: initial.mass_storage.cap.value(),
                energy: initial.energy_storage.cap.value(),
            },));
            world.insert_resource(EcoState(initial));
        }

        Self { app }
    }

    /// Advance the simulation by one `dt` and return the events produced.
    pub fn step(&mut self) -> &[SimulationEvent] {
        {
            let mut journal = self.app.world_mut().resource_mut::<EventJournal>();
            journal.0.clear();
        }

        if self.is_finished() {
            return &[];
        }

        self.app.update();

        let journal = self.app.world().resource::<EventJournal>();
        &journal.0
    }

    /// True if the queue has finished or the simulation hit `max_time`.
    pub fn is_finished(&self) -> bool {
        self.app.world().resource::<FinishedFlag>().0
    }

    /// Current simulation time in seconds.
    pub fn current_time(&self) -> f64 {
        self.app.world().resource::<SimClock>().time
    }
}
