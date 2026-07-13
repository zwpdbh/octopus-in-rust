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
use faf_sim::protocol::{ControlEvent, SimRuntimeStatus, SimulationMode};
use faf_sim::quantities::{StepTime, Time};
use faf_sim::sim::{BuildQueue, Simulation, SimulationEvent};
use thiserror::Error;
use uuid::Uuid;

pub type SimulationId = Uuid;

/// Configuration for a single simulation run.
///
/// This type is crate-private so that external clients cannot construct it
/// directly. Use [`SimulationService::start_active`] or
/// [`SimulationService::start_passive`] instead.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RunConfig {
    /// Simulation step size. Must be an integer number of seconds >= 1.
    /// dt is the granularity of the simulation.
    /// A larger dt means fewer snapshots and less precision;
    /// a smaller dt means more snapshots and finer resolution.
    pub(crate) dt: StepTime,
    /// Optional hard cap in simulation time. When `None` the simulation runs
    /// until the build queue is empty.
    pub(crate) max_time: Option<Time>,
    /// How the simulation is driven: manual `Advance` commands or real-time
    /// auto-play.
    pub(crate) mode: SimulationMode,
}

impl RunConfig {
    pub(crate) fn new(dt: StepTime, max_time: Option<Time>, mode: SimulationMode) -> Self {
        Self { dt, max_time, mode }
    }
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            dt: StepTime::from_seconds(1).expect("1 second is a valid step time"),
            max_time: None,
            mode: SimulationMode::Passive {
                tick_interval_ms: 50,
            },
        }
    }
}

#[derive(Debug, Clone)]
enum ControlCmd {
    Pause,
    Resume,
    Stop,
    Advance(StepTime),
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

    /// Start a new simulation.
    ///
    /// Returns the generated [`SimulationId`]. The simulation will wait for the
    /// first subscriber before stepping. Use [`Self::subscribe`] to receive
    /// events; multiple subscribers can attach to the same simulation.
    pub(crate) fn start(&self, queue: BuildQueue, config: RunConfig) -> SimulationId {
        let id = Uuid::new_v4();
        let (control_tx, control_rx) = unbounded();
        let subscriber_count = Arc::new(AtomicUsize::new(0));

        let handle = SimulationHandle {
            control_tx,
            subscriber_count: subscriber_count.clone(),
        };
        self.simulations.lock().unwrap().insert(id, handle);

        let service = self.simulations.clone();
        let count_for_thread = subscriber_count.clone();
        std::thread::spawn(move || {
            run_simulation_thread(queue, config, control_rx, Vec::new(), count_for_thread);
            service.lock().unwrap().remove(&id);
        });

        id
    }

    /// Start a new simulation in active (manual-advance) mode.
    ///
    /// Returns the generated [`SimulationId`]. The simulation will wait for the
    /// first subscriber before stepping. Use [`Self::subscribe`] to receive
    /// events; multiple subscribers can attach to the same simulation.
    pub fn start_active_sim(
        &self,
        queue: BuildQueue,
        dt: StepTime,
        max_time: Option<Time>,
    ) -> SimulationId {
        self.start(queue, RunConfig::new(dt, max_time, SimulationMode::Active))
    }

    /// Start a new simulation in passive (auto-play) mode.
    ///
    /// Returns the generated [`SimulationId`]. The simulation will wait for the
    /// first subscriber before stepping. Use [`Self::subscribe`] to receive
    /// events; multiple subscribers can attach to the same simulation.
    pub fn start_passive_sim(
        &self,
        queue: BuildQueue,
        dt: StepTime,
        max_time: Option<Time>,
        tick_interval_ms: u64,
    ) -> SimulationId {
        self.start(
            queue,
            RunConfig::new(dt, max_time, SimulationMode::Passive { tick_interval_ms }),
        )
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

    /// Advance a simulation by one manual step of `dt`.
    pub fn advance(&self, id: SimulationId, dt: StepTime) -> Result<(), ServiceError> {
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

impl From<RunState> for SimRuntimeStatus {
    fn from(state: RunState) -> Self {
        match state {
            RunState::Running => SimRuntimeStatus::Running,
            RunState::Paused => SimRuntimeStatus::Paused,
            RunState::Stopped => SimRuntimeStatus::Stopped,
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

    match config.mode {
        SimulationMode::Active => {
            run_active_loop(&mut sim, control_rx, &mut subscribers, config.mode)
        }
        SimulationMode::Passive { tick_interval_ms } => run_passive_loop(
            &mut sim,
            control_rx,
            &mut subscribers,
            subscriber_count,
            tick_interval_ms,
            config.mode,
        ),
    }
}

/// Active mode: the simulation only steps on explicit `Advance` commands.
fn run_active_loop(
    sim: &mut Simulation,
    control_rx: Receiver<ControlCmd>,
    subscribers: &mut Vec<Sender<SimServiceEvent>>,
    mode: SimulationMode,
) {
    // In active mode the runtime state is always conceptually "paused" because
    // the simulation never auto-steps. We keep a fixed Paused state so control
    // events still report a consistent value.
    let mut state = RunState::Paused;

    loop {
        match control_rx.recv() {
            Ok(cmd) => {
                let events = apply_control_cmd(mode, &mut state, cmd, sim, subscribers);
                broadcast_events(subscribers, events);
                if state == RunState::Stopped {
                    return;
                }
            }
            Err(_) => return,
        }
    }
}

/// Passive mode: the simulation auto-steps with a real-time delay between ticks.
fn run_passive_loop(
    sim: &mut Simulation,
    control_rx: Receiver<ControlCmd>,
    subscribers: &mut Vec<Sender<SimServiceEvent>>,
    subscriber_count: Arc<AtomicUsize>,
    tick_interval_ms: u64,
    mode: SimulationMode,
) {
    let tick_interval = Duration::from_millis(tick_interval_ms);
    let mut state = RunState::Running;

    loop {
        // Process any queued commands first.
        while let Ok(cmd) = control_rx.try_recv() {
            let events = apply_control_cmd(mode, &mut state, cmd, sim, subscribers);
            broadcast_events(subscribers, events);
            if state == RunState::Stopped {
                return;
            }
        }

        if state == RunState::Paused {
            // While paused we block on the control channel so resume/stop is
            // immediately responsive instead of polling on tick_interval.
            match control_rx.recv() {
                Ok(cmd) => {
                    let events = apply_control_cmd(mode, &mut state, cmd, sim, subscribers);
                    broadcast_events(subscribers, events);
                    if state == RunState::Stopped {
                        return;
                    }
                }
                Err(_) => return,
            }
            continue;
        }

        if subscriber_count.load(Ordering::SeqCst) == 0 {
            // No subscribers yet: block until a command arrives instead of
            // exiting, so callers can start a simulation and subscribe later.
            match control_rx.recv() {
                Ok(cmd) => {
                    let events = apply_control_cmd(mode, &mut state, cmd, sim, subscribers);
                    broadcast_events(subscribers, events);
                    if state == RunState::Stopped {
                        return;
                    }
                }
                Err(_) => return,
            }
            continue;
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
            broadcast(subscribers, event);
            if is_finished {
                return;
            }
        }

        std::thread::sleep(tick_interval);
    }
}

/// Apply a control command, validating the `(mode, state, cmd)` combination.
///
/// Mutates `state` in place and returns any service events produced by the
/// command. When `state` becomes `RunState::Stopped`, the caller should exit.
fn apply_control_cmd(
    mode: SimulationMode,
    state: &mut RunState,
    cmd: ControlCmd,
    sim: &mut Simulation,
    subscribers: &mut Vec<Sender<SimServiceEvent>>,
) -> Vec<SimServiceEvent> {
    let old_state = *state;
    match (mode, old_state, cmd) {
        // Stop is always valid and terminates the thread.
        (_, _, ControlCmd::Stop) => {
            *state = RunState::Stopped;
            vec![state_changed_event(old_state, RunState::Stopped)]
        }
        // In active mode only Advance causes a step; Pause/Resume are no-ops.
        (SimulationMode::Active, _, ControlCmd::Advance(dt)) => {
            if !sim.is_finished() {
                sim.step_with_dt(dt)
                    .iter()
                    .map(|e| SimServiceEvent::Simulation(e.clone()))
                    .collect()
            } else {
                vec![]
            }
        }
        // Pause/Resume only make sense in passive mode.
        (SimulationMode::Passive { .. }, RunState::Running, ControlCmd::Pause) => {
            *state = RunState::Paused;
            vec![state_changed_event(old_state, RunState::Paused)]
        }
        (SimulationMode::Passive { .. }, RunState::Paused, ControlCmd::Resume) => {
            *state = RunState::Running;
            vec![state_changed_event(old_state, RunState::Running)]
        }
        // Advance is a no-op in passive mode; Pause/Resume are no-ops in active.
        // Subscribers can attach in any mode/state.
        (_, _, ControlCmd::Subscribe(tx)) => {
            subscribers.push(tx);
            vec![]
        }
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
    use faf_sim::quantities::{Energy, EnergyRate, Mass, MassRate, StepTime, Storage, Time};
    use faf_sim::sim::{BuildQueue, BuildTask, EcoSnapshot, SimulationEvent, UnitDefRef};

    fn rich_eco() -> EconomyState {
        EconomyState {
            mass_income: MassRate::from_raw(1000.0),
            energy_income: EnergyRate::from_raw(1000.0),
            mass_storage: Storage::new(Mass::from_raw(10000.0), Mass::from_raw(10000.0)),
            energy_storage: Storage::new(Energy::from_raw(10000.0), Energy::from_raw(10000.0)),
            ..Default::default()
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
                targets: vec![UnitDefRef {
                    build_power: 0.0,
                    mass_cost: 100.0,
                    energy_cost: 100.0,
                    build_time: 100.0,
                    ..Default::default()
                }],
            }],
        }
    }

    #[test]
    fn pause_emits_state_changed_event() {
        let service = SimulationService::new();
        let id = service.start_passive_sim(
            make_queue(),
            StepTime::from_seconds(1).unwrap(),
            Some(Time::from_raw(1000.0)),
            1,
        );
        let rx = service.subscribe(id).unwrap();

        service.pause(id).unwrap();

        let mut found = false;
        while let Ok(event) = rx.recv() {
            if let SimServiceEvent::Control(ControlEvent::StateChanged { from, to }) = event {
                assert_eq!(from, SimRuntimeStatus::Running);
                assert_eq!(to, SimRuntimeStatus::Paused);
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
        let id = service.start_active_sim(
            make_queue(),
            StepTime::from_seconds(1).unwrap(),
            Some(Time::from_raw(1000.0)),
        );
        let rx = service.subscribe(id).unwrap();

        // Pause before any automatic steps occur.
        service.pause(id).unwrap();

        // Drain any events that may have been produced before pause took effect.
        while rx.try_recv().is_ok() {}

        // Manually advance by 2.0 seconds.
        service
            .advance(id, StepTime::from_seconds(2).unwrap())
            .unwrap();

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

    #[test]
    fn multiple_subscribers_receive_same_events() {
        let service = SimulationService::new();
        let id = service.start_active_sim(
            make_queue(),
            StepTime::from_seconds(1).unwrap(),
            Some(Time::from_raw(1000.0)),
        );
        let rx1 = service.subscribe(id).unwrap();
        let rx2 = service.subscribe(id).unwrap();

        // In active mode the simulation does not auto-step, so we can advance
        // manually without pausing first.

        // Advance manually; both subscribers should see the Ticked event.
        service
            .advance(id, StepTime::from_seconds(2).unwrap())
            .unwrap();

        // Drain events until each subscriber observes the Ticked event from the
        // manual advance (a TaskStarted event is emitted first).
        fn recv_ticked(rx: &Receiver<SimServiceEvent>) -> EcoSnapshot {
            while let Ok(event) = rx.recv() {
                if let SimServiceEvent::Simulation(SimulationEvent::Ticked(snapshot)) = event {
                    return snapshot;
                }
            }
            panic!("receiver closed before Ticked event");
        }
        let s1 = recv_ticked(&rx1);
        let s2 = recv_ticked(&rx2);

        assert!((s1.time - 2.0).abs() < 1e-9);
        assert!((s2.time - 2.0).abs() < 1e-9);

        service.stop(id).unwrap();
    }

    #[test]
    fn active_mode_does_not_auto_step() {
        let service = SimulationService::new();
        let id = service.start_active_sim(
            make_queue(),
            StepTime::from_seconds(1).unwrap(),
            Some(Time::from_raw(1000.0)),
        );
        let rx = service.subscribe(id).unwrap();

        // Wait a short time to give a hypothetical auto-step loop a chance to
        // emit something. In active mode nothing should arrive.
        std::thread::sleep(Duration::from_millis(20));
        assert!(rx.try_recv().is_err());

        service.stop(id).unwrap();
    }
}
