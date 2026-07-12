//! Multi-simulation service that wraps [`faf_sim::Simulation`] into shareable
//! event streams.
//!
//! A [`SimulationService`] owns any number of concurrent simulations keyed by
//! [`SimulationId`]. Each simulation runs on a dedicated OS thread because the
//! underlying Bevy `App` is not `Send`. Subscribers receive events through a
//! [`crossbeam_channel`] receiver. When the last subscriber drops, the
//! simulation stops automatically.

use std::collections::HashMap;
use std::ops::Deref;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam_channel::{unbounded, Receiver, Sender};
use faf_sim::protocol::{ControlEvent, SimulationState};
use faf_sim::sim::{BuildQueue, Simulation, SimulationEvent};
use thiserror::Error;
use uuid::Uuid;

pub type SimulationId = Uuid;

/// Configuration for a single simulation run.
#[derive(Debug, Clone, Copy)]
pub struct RunConfig {
    /// Simulation step size in seconds.
    pub dt: f64,
    /// Optional hard cap in seconds. When `None` the simulation runs until the
    /// build queue is empty.
    pub max_time: Option<f64>,
    /// Real-world delay between simulation steps.
    pub tick_interval: Duration,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            dt: 0.1,
            max_time: None,
            tick_interval: Duration::from_millis(50),
        }
    }
}

#[derive(Debug, Clone)]
enum ControlCmd {
    Pause,
    Resume,
    Stop,
    Advance(f64),
    Subscribe(Sender<SimServiceEvent>),
}

/// Event emitted by the simulation service.
///
/// This is a sum type: clients receive either a raw simulation event produced
/// by a step, or a control event produced by a command.
#[derive(Debug, Clone)]
pub enum SimServiceEvent {
    Simulation(SimulationEvent),
    Control(ControlEvent),
}

struct SimulationHandle {
    control_tx: Sender<ControlCmd>,
    subscriber_count: Arc<AtomicUsize>,
}

/// Error returned by [`SimulationService`] operations.
#[derive(Debug, Error)]
pub enum ServiceError {
    /// The requested simulation does not exist.
    #[error("simulation {0} not found")]
    NotFound(SimulationId),
}

/// Receiver for a simulation event stream.
///
/// Derefs to a [`Receiver<SimServiceEvent>`] and decrements the subscriber
/// count when dropped. When the subscriber count reaches zero the simulation
/// thread exits automatically.
pub struct SimulationReceiver {
    rx: Receiver<SimServiceEvent>,
    subscriber_count: Arc<AtomicUsize>,
}

impl Deref for SimulationReceiver {
    type Target = Receiver<SimServiceEvent>;

    fn deref(&self) -> &Self::Target {
        &self.rx
    }
}

impl Drop for SimulationReceiver {
    fn drop(&mut self) {
        self.subscriber_count.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Service that manages multiple concurrent simulations.
#[derive(Default, Clone)]
pub struct SimulationService {
    simulations: Arc<Mutex<HashMap<SimulationId, SimulationHandle>>>,
}

impl SimulationService {
    /// Create an empty service.
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a new simulation and subscribe to its event stream.
    ///
    /// Returns the generated [`SimulationId`] and a receiver for the first
    /// subscriber. Additional subscribers can attach with [`Self::subscribe`].
    pub fn start(
        &self,
        queue: BuildQueue,
        config: RunConfig,
    ) -> (SimulationId, SimulationReceiver) {
        let id = Uuid::new_v4();
        let (control_tx, control_rx) = unbounded();
        let subscriber_count = Arc::new(AtomicUsize::new(0));
        let (event_tx, event_rx) = unbounded();

        subscriber_count.fetch_add(1, Ordering::SeqCst);

        let handle = SimulationHandle {
            control_tx,
            subscriber_count: subscriber_count.clone(),
        };
        self.simulations.lock().unwrap().insert(id, handle);

        let service = self.simulations.clone();
        let count_for_thread = subscriber_count.clone();
        std::thread::spawn(move || {
            run_simulation_thread(queue, config, control_rx, vec![event_tx], count_for_thread);
            service.lock().unwrap().remove(&id);
        });

        let receiver = SimulationReceiver {
            rx: event_rx,
            subscriber_count,
        };
        (id, receiver)
    }

    /// Subscribe to an existing simulation.
    ///
    /// Returns a new receiver that will receive events produced from now on.
    pub fn subscribe(&self, id: SimulationId) -> Result<SimulationReceiver, ServiceError> {
        let sims = self.simulations.lock().unwrap();
        let handle = sims.get(&id).ok_or(ServiceError::NotFound(id))?;
        handle.subscriber_count.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = unbounded();
        let subscriber_count = handle.subscriber_count.clone();
        // The simulation thread will pick up the new sender on its next loop.
        // We cannot add it here because the thread owns the subscribers list.
        // Instead, we send it through the control channel.
        let _ = handle.control_tx.send(ControlCmd::Subscribe(tx));
        drop(sims);

        Ok(SimulationReceiver {
            rx,
            subscriber_count,
        })
    }

    /// Pause a running simulation.
    pub fn pause(&self, id: SimulationId) -> Result<(), ServiceError> {
        self.send_control(id, ControlCmd::Pause)
    }

    /// Resume a paused simulation.
    pub fn resume(&self, id: SimulationId) -> Result<(), ServiceError> {
        self.send_control(id, ControlCmd::Resume)
    }

    /// Stop a simulation and drop it from the service.
    pub fn stop(&self, id: SimulationId) -> Result<(), ServiceError> {
        self.send_control(id, ControlCmd::Stop)
    }

    /// Advance a simulation by one manual step of `dt` simulation seconds.
    pub fn advance(&self, id: SimulationId, dt: f64) -> Result<(), ServiceError> {
        self.send_control(id, ControlCmd::Advance(dt))
    }

    fn send_control(&self, id: SimulationId, cmd: ControlCmd) -> Result<(), ServiceError> {
        let sims = self.simulations.lock().unwrap();
        let handle = sims.get(&id).ok_or(ServiceError::NotFound(id))?;
        let _ = handle.control_tx.send(cmd);
        Ok(())
    }
}

/// Runtime state of a simulation managed by the service.
///
/// This is distinct from [`Simulation::is_finished`], which reflects whether
/// the build queue has been exhausted. `RunState` tracks whether the service
/// thread is currently auto-stepping or waiting for manual commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunState {
    Running,
    Paused,
    Stopped,
}

impl From<RunState> for SimulationState {
    fn from(state: RunState) -> Self {
        match state {
            RunState::Running => SimulationState::Running,
            RunState::Paused => SimulationState::Paused,
            RunState::Stopped => SimulationState::Stopped,
        }
    }
}

fn run_simulation_thread(
    queue: BuildQueue,
    config: RunConfig,
    control_rx: Receiver<ControlCmd>,
    initial_subscribers: Vec<Sender<SimServiceEvent>>,
    subscriber_count: Arc<AtomicUsize>,
) {
    let mut sim = Simulation::new(queue, config.dt, config.max_time);
    let mut subscribers = initial_subscribers;
    let mut state = RunState::Running;

    loop {
        // Process any queued commands first.
        while let Ok(cmd) = control_rx.try_recv() {
            let events = apply_control_cmd(&mut state, cmd, &mut sim, &mut subscribers);
            broadcast_events(&mut subscribers, events);
            if state == RunState::Stopped {
                return;
            }
        }

        if state == RunState::Paused {
            // While paused we block on the control channel so manual steps are
            // immediately responsive instead of polling on tick_interval.
            match control_rx.recv() {
                Ok(cmd) => {
                    let events = apply_control_cmd(&mut state, cmd, &mut sim, &mut subscribers);
                    broadcast_events(&mut subscribers, events);
                    if state == RunState::Stopped {
                        return;
                    }
                }
                Err(_) => return,
            }
            continue;
        }

        if subscriber_count.load(Ordering::SeqCst) == 0 {
            return;
        }

        if sim.is_finished() {
            return;
        }

        for event in sim.step() {
            let event = SimServiceEvent::Simulation(event.clone());
            let is_finished = matches!(
                &event,
                SimServiceEvent::Simulation(SimulationEvent::Finished)
            );
            broadcast(&mut subscribers, event);
            if is_finished {
                return;
            }
        }

        std::thread::sleep(config.tick_interval);
    }
}

/// Apply a control command, validating the `(state, cmd)` combination.
///
/// Mutates `state` in place and returns any service events produced by the
/// command. When `state` becomes `RunState::Stopped`, the caller should exit.
fn apply_control_cmd(
    state: &mut RunState,
    cmd: ControlCmd,
    sim: &mut Simulation,
    subscribers: &mut Vec<Sender<SimServiceEvent>>,
) -> Vec<SimServiceEvent> {
    let old_state = *state;
    match (old_state, cmd) {
        // Pause only makes sense while running.
        (RunState::Running, ControlCmd::Pause) => {
            *state = RunState::Paused;
            vec![state_changed_event(old_state, RunState::Paused)]
        }
        // Resume only makes sense while paused.
        (RunState::Paused, ControlCmd::Resume) => {
            *state = RunState::Running;
            vec![state_changed_event(old_state, RunState::Running)]
        }
        // Stop is always valid and terminates the thread.
        (_, ControlCmd::Stop) => {
            *state = RunState::Stopped;
            vec![state_changed_event(old_state, RunState::Stopped)]
        }
        // Manual advance is only valid while paused.
        (RunState::Paused, ControlCmd::Advance(dt)) => {
            if dt > 0.0 && !sim.is_finished() {
                sim.step_with_dt(dt)
                    .iter()
                    .map(|e| SimServiceEvent::Simulation(e.clone()))
                    .collect()
            } else {
                vec![]
            }
        }
        // Subscribers can attach in any state.
        (_, ControlCmd::Subscribe(tx)) => {
            subscribers.push(tx);
            vec![]
        }
        // All other combinations are no-ops (e.g. Resume while running, Pause
        // while paused, Advance while running).
        _ => vec![],
    }
}

fn state_changed_event(from: RunState, to: RunState) -> SimServiceEvent {
    SimServiceEvent::Control(ControlEvent::StateChanged {
        from: from.into(),
        to: to.into(),
    })
}

fn broadcast(subscribers: &mut Vec<Sender<SimServiceEvent>>, event: SimServiceEvent) {
    subscribers.retain(|tx| tx.send(event.clone()).is_ok());
}

fn broadcast_events(subscribers: &mut Vec<Sender<SimServiceEvent>>, events: Vec<SimServiceEvent>) {
    for event in events {
        if let SimServiceEvent::Simulation(SimulationEvent::Finished) = &event {
            broadcast(subscribers, event);
            return;
        }
        broadcast(subscribers, event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use faf_sim::economy::EconomyState;
    use faf_sim::quantities::{Energy, EnergyRate, Mass, MassRate, Storage, Time};
    use faf_sim::sim::{BuildQueue, BuildTask, SimulationEvent, UnitDefRef};

    fn rich_eco() -> EconomyState {
        EconomyState {
            net_mass_income: MassRate::from_raw(1000.0),
            net_energy_income: EnergyRate::from_raw(1000.0),
            mass_storage: Storage::new(Mass::from_raw(10000.0), Mass::from_raw(10000.0)),
            energy_storage: Storage::new(Energy::from_raw(10000.0), Energy::from_raw(10000.0)),
        }
    }

    fn make_queue() -> BuildQueue {
        BuildQueue {
            initial_eco: rich_eco(),
            tasks: vec![BuildTask {
                id: 1,
                start_after: Time::from_raw(0.0),
                builders: vec![UnitDefRef {
                    build_power: 10.0,
                    ..Default::default()
                }],
                target: UnitDefRef {
                    build_power: 0.0,
                    mass_cost: 100.0,
                    energy_cost: 100.0,
                    build_time: 100.0,
                    ..Default::default()
                },
            }],
        }
    }

    #[test]
    fn pause_emits_state_changed_event() {
        let service = SimulationService::new();
        let config = RunConfig {
            dt: 1.0,
            max_time: Some(1000.0),
            tick_interval: Duration::from_millis(1),
        };
        let (id, rx) = service.start(make_queue(), config);

        service.pause(id).unwrap();

        let mut found = false;
        while let Ok(event) = rx.recv() {
            if let SimServiceEvent::Control(ControlEvent::StateChanged { from, to }) = event {
                assert_eq!(from, SimulationState::Running);
                assert_eq!(to, SimulationState::Paused);
                found = true;
                break;
            }
        }
        assert!(found);

        service.stop(id).unwrap();
    }

    #[test]
    fn advance_while_paused_produces_tick_event() {
        let service = SimulationService::new();
        let config = RunConfig {
            dt: 1.0,
            max_time: Some(1000.0),
            tick_interval: Duration::from_millis(1),
        };
        let (id, rx) = service.start(make_queue(), config);

        // Pause before any automatic steps occur.
        service.pause(id).unwrap();

        // Drain any events that may have been produced before pause took effect.
        while rx.try_recv().is_ok() {}

        // Manually advance by 2.0 seconds.
        service.advance(id, 2.0).unwrap();

        // We should receive at least one Ticked event with time == 2.0.
        let mut found = false;
        while let Ok(event) = rx.recv() {
            if let SimServiceEvent::Simulation(SimulationEvent::Ticked(snapshot)) = event {
                assert!((snapshot.time - 2.0).abs() < 1e-9);
                found = true;
                break;
            }
            if matches!(
                event,
                SimServiceEvent::Simulation(SimulationEvent::Finished)
            ) {
                break;
            }
        }
        assert!(found);

        service.stop(id).unwrap();
    }
}
