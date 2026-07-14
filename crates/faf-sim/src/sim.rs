//! Simulation entry point.
//!
//! This module provides [`Simulation`], the high-level synchronous driver that
//! owns a Bevy `App`, wires in [`EcoPlugin`](crate::runtime::EcoPlugin), and
//! lets callers step the simulation one tick at a time.
//!
//! The input/output types are defined in [`crate::runtime`] and re-exported at
//! the crate root so consumers have a single obvious import path.

use bevy_app::prelude::*;

use crate::quantities::{StepTime, Time};
use crate::runtime::components::{Producer, StorageContributor};
use crate::runtime::resources::{
    CompletedTasks, EcoState, EffectiveFactor, EventJournal, FinishedFlag, PendingTasks, SimClock,
    TailEndTime, TotalsSpent,
};
pub use crate::runtime::{
    BuildQueue, BuildTask, EcoPlugin, EcoSnapshot, SimulationEvent, UnitEcoStats,
};

/// Steppable economy simulation.
pub struct Simulation {
    app: App,
    dt: StepTime,
}

impl Simulation {
    /// Create a new simulation for the given queue.
    ///
    /// `dt` is the validated simulation step size (integer seconds, at least 1).
    /// `max_time` is an optional hard cap in seconds; when `None` the simulation
    /// runs until the build queue is empty.
    pub fn new(queue: BuildQueue, dt: StepTime, max_time: Option<Time>) -> Self {
        let mut app = App::new();
        app.add_plugins(EcoPlugin)
            .insert_resource(SimClock {
                time: Time::from_raw(0.0),
                dt: dt.as_time(),
                max_time,
            })
            .insert_resource(PendingTasks::from_tasks(queue.tasks))
            .insert_resource(CompletedTasks(Vec::new()))
            .insert_resource(EffectiveFactor(1.0))
            .insert_resource(EventJournal::default())
            .insert_resource(FinishedFlag::default())
            .insert_resource(TotalsSpent {
                mass: 0.0,
                energy: 0.0,
            })
            .insert_resource(TailEndTime::default());

        // Seed the world with the initial economy so recompute_base_economy_system
        // preserves the caller's starting income and storage capacity.
        {
            let initial = queue.initial_eco;
            let world = app.world_mut();
            world.spawn((Producer {
                production_per_second_mass: initial.production_per_second_mass.value(),
                production_per_second_energy: initial.production_per_second_energy.value(),
                maintenance_consumption_per_second_energy: initial
                    .maintenance_consumption_per_second_energy
                    .value(),
            },));
            world.spawn((StorageContributor {
                mass: initial.mass_storage.cap.value(),
                energy: initial.energy_storage.cap.value(),
            },));
            world.insert_resource(EcoState(initial));
        }

        Self { app, dt }
    }

    /// Advance the simulation by one `dt` and return the events produced.
    pub fn step(&mut self) -> &[SimulationEvent] {
        self.step_with_dt(self.dt)
    }

    /// Advance the simulation by one step of the requested size and return the
    /// events produced.
    ///
    /// The configured step size is restored after the update so subsequent
    /// [`Self::step`] calls continue to use the original `dt`.
    pub fn step_with_dt(&mut self, dt: StepTime) -> &[SimulationEvent] {
        {
            let mut journal = self.app.world_mut().resource_mut::<EventJournal>();
            journal.0.clear();
        }

        if self.is_finished() {
            return &[];
        }

        {
            let mut clock = self.app.world_mut().resource_mut::<SimClock>();
            clock.dt = dt.as_time();
        }

        self.app.update();

        {
            let mut clock = self.app.world_mut().resource_mut::<SimClock>();
            clock.dt = self.dt.as_time();
        }

        let journal = self.app.world().resource::<EventJournal>();
        &journal.0
    }

    /// True if the queue has finished or the simulation hit `max_time`.
    pub fn is_finished(&self) -> bool {
        self.app.world().resource::<FinishedFlag>().0
    }

    /// Current simulation time.
    pub fn current_time(&self) -> Time {
        self.app.world().resource::<SimClock>().time
    }
}
