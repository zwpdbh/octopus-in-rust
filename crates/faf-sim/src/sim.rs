//! Simulation entry point.
//!
//! This module provides [`Simulation`], the high-level driver that owns a Bevy
//! `App`, wires in the [`EcoPlugin`](crate::eco::EcoPlugin), and lets callers
//! step the simulation one tick at a time.
//!
//! It also provides [`SimulationService`], which wraps a single simulation
//! instance with command support (start, pause, resume, stop) and a listener
//! registry so multiple clients can receive the same event stream.
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
    /// `dt` is the simulation step size in seconds. `max_time` is a hard cap
    /// to prevent infinite simulation.
    pub fn new(queue: BuildQueue, dt: f64, max_time: f64) -> Self {
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

/// Current state of a [`SimulationService`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    /// No simulation is loaded.
    Idle,
    /// The simulation is actively stepping.
    Running,
    /// The simulation is paused and can be resumed.
    Paused,
    /// The simulation has reached its natural end or `max_time`.
    Finished,
}

/// Commands that can be sent to a [`SimulationService`].
#[derive(Debug, Clone, PartialEq)]
pub enum SimulationCommand {
    /// Start (or restart) a simulation.
    Start {
        queue: BuildQueue,
        dt: f64,
        max_time: f64,
    },
    /// Pause a running simulation.
    Pause,
    /// Resume a paused simulation.
    Resume,
    /// Stop the current simulation and return to idle.
    Stop,
    /// Step the simulation forward by `steps` `dt`s.
    Tick { steps: usize },
}

/// Internal state machine. Carrying the [`Simulation`] in the variant makes
/// invalid combinations like "Running without a simulation" unrepresentable.
enum ServiceStateInternal {
    Idle,
    Running(Simulation),
    Paused(Simulation),
    Finished,
}

impl ServiceStateInternal {
    fn kind(&self) -> ServiceState {
        match self {
            ServiceStateInternal::Idle => ServiceState::Idle,
            ServiceStateInternal::Running(_) => ServiceState::Running,
            ServiceStateInternal::Paused(_) => ServiceState::Paused,
            ServiceStateInternal::Finished => ServiceState::Finished,
        }
    }
}

/// Shared simulation instance that receives commands and broadcasts events.
///
/// Multiple clients can register listeners. The same event stream is delivered
/// to every listener, making this suitable for feeding both a UI chart and a
/// logger from one simulation.
///
/// The service does not run on its own thread. Callers are responsible for
/// sending [`SimulationCommand::Tick`] repeatedly (e.g. from a timer, a game
/// loop, or a background task).
pub struct SimulationService {
    state: ServiceStateInternal,
    listeners: Vec<Box<dyn Fn(&SimulationEvent)>>,
}

impl SimulationService {
    /// Create an idle service with no listeners.
    pub fn new() -> Self {
        Self {
            state: ServiceStateInternal::Idle,
            listeners: Vec::new(),
        }
    }

    /// Current service state.
    pub fn state(&self) -> ServiceState {
        self.state.kind()
    }

    /// Register a listener that will be called for every emitted event.
    pub fn register_listener(&mut self, listener: impl Fn(&SimulationEvent) + 'static) {
        self.listeners.push(Box::new(listener));
    }

    /// Handle a command and update the service state.
    ///
    /// Valid combinations are handled explicitly; everything else is a no-op.
    pub fn send(&mut self, command: SimulationCommand) {
        let current = std::mem::replace(&mut self.state, ServiceStateInternal::Idle);

        self.state = match (current, command) {
            // Start/restart is allowed from Idle or Finished.
            (
                ServiceStateInternal::Idle | ServiceStateInternal::Finished,
                SimulationCommand::Start {
                    queue,
                    dt,
                    max_time,
                },
            ) => ServiceStateInternal::Running(Simulation::new(queue, dt, max_time)),
            (ServiceStateInternal::Running(sim), SimulationCommand::Pause) => {
                ServiceStateInternal::Paused(sim)
            }
            (ServiceStateInternal::Paused(sim), SimulationCommand::Resume) => {
                ServiceStateInternal::Running(sim)
            }
            (
                ServiceStateInternal::Running(_)
                | ServiceStateInternal::Paused(_)
                | ServiceStateInternal::Finished,
                SimulationCommand::Stop,
            ) => ServiceStateInternal::Idle,
            (ServiceStateInternal::Running(mut sim), SimulationCommand::Tick { steps }) => {
                let steps = steps.max(1);
                for _ in 0..steps {
                    let events: Vec<SimulationEvent> = sim.step().to_vec();
                    for event in &events {
                        for listener in &self.listeners {
                            listener(event);
                        }
                    }
                }

                if sim.is_finished() {
                    ServiceStateInternal::Finished
                } else {
                    ServiceStateInternal::Running(sim)
                }
            }
            // All other (state, command) pairs are invalid; keep the previous state.
            (current, _) => current,
        };
    }
}

impl Default for SimulationService {
    fn default() -> Self {
        Self::new()
    }
}
