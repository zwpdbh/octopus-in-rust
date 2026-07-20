//! Simulation entry point.
//!
//! This module provides [`Simulation`], the high-level synchronous driver that
//! owns a Bevy `App`, wires in [`BuildQueueSimulationPlugin`](crate::runtime::BuildQueueSimulationPlugin), and
//! lets callers step the simulation one tick at a time.
//!
//! The input/output types are defined in [`crate::runtime`] and re-exported at
//! the crate root so consumers have a single obvious import path.

use bevy_app::prelude::*;

use crate::quantities::{StepTime, Time};
use crate::runtime::resources::{
    CompletedTasks, EcoState, EffectiveFactor, EventJournal, FinishedFlag, PendingTasks,
    PostQueueTailSeconds, SimClock, TailEndTime, TotalsSpent,
};
pub use crate::runtime::{
    AdjacencyBonus, BuildQueue, BuildQueueSimulationPlugin, BuildTask, EcoSnapshot,
    SimulationEvent, UnitEcoStats,
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
    /// `tail_seconds` is an optional post-queue tail: when `Some(seconds)` the
    /// simulation keeps ticking for that long after the queue empties so the
    /// final economy state remains visible in charts. Pass `None` to finish
    /// immediately when the queue is empty.
    pub fn new(
        queue: BuildQueue,
        dt: StepTime,
        max_time: Option<Time>,
        tail_seconds: Option<f64>,
    ) -> Self {
        let mut app = App::new();
        app.add_plugins(BuildQueueSimulationPlugin)
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
            .insert_resource(TailEndTime::default())
            .insert_resource(PostQueueTailSeconds(tail_seconds))
            .insert_resource(EcoState(queue.initial_eco));

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
