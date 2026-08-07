use crate::sim_systems::*;
use bevy_app::prelude::*;
use faf_blueprints::{ConstructionAction, ConstructionPlan};
use faf_game_engine::*;
use faf_sim_protocol::{SimCmd, SimEvent, SimSpeed};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};
use uuid::Uuid;

pub type SimulationId = Uuid;

#[derive(Debug, thiserror::Error)]
pub enum SimulationError {
    #[error("simulation thread panicked")]
    ThreadPanicked,
}

/// Handle used by the service to send control commands to a simulation thread.
#[derive(Debug)]
struct SimulationHandle {
    #[allow(unused)]
    sim_tx: crossbeam_channel::Sender<SimCmd>,
}

/// Registry of all simulations currently running in background threads.
#[derive(Debug, Default)]
pub struct SimulationService {
    simulations: Arc<Mutex<HashMap<SimulationId, SimulationHandle>>>,
}

impl SimulationService {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a simulation in a background thread at the given speed.
    pub fn run(&self, construction_plan: ConstructionPlan, speed: SimSpeed) {
        let _ = self.spawn_run(construction_plan, speed);
    }

    /// Start a simulation and block until it completes.
    pub fn run_blocking(
        &self,
        construction_plan: ConstructionPlan,
        speed: SimSpeed,
    ) -> Result<(), SimulationError> {
        let handle = self.spawn_run(construction_plan, speed);
        handle.join().map_err(|_| SimulationError::ThreadPanicked)?;
        Ok(())
    }

    fn spawn_run(&self, construction_plan: ConstructionPlan, speed: SimSpeed) -> JoinHandle<()> {
        let simulation_id = Uuid::new_v4();
        let (sim_tx, sim_rx) = crossbeam_channel::unbounded::<SimCmd>();

        let sim_handle = SimulationHandle { sim_tx };
        let service = self.simulations.clone();

        self.simulations
            .lock()
            .unwrap()
            .insert(simulation_id, sim_handle);

        std::thread::spawn(move || {
            run_sim_thread(construction_plan, sim_rx, speed);
            service.lock().unwrap().remove(&simulation_id);
        })
    }
}

#[derive(Debug, PartialEq)]
enum SimRunState {
    Running,
    Paused,
    Stopped,
}

/// Main simulation loop.
///
/// Runs in its own OS thread.  It owns the Bevy `App`, seeds it from the
/// `ConstructionPlan`, and then repeatedly:
///
/// 1. Drains external `SimCmd`s (pause, resume, speed change).
/// 2. Drains outgoing `SimEvent`s from the engine and prints them.
/// 3. Steps the engine by one tick (`app.update()`).
/// 4. Throttles the loop to match the requested `SimSpeed`.
///
/// Actions are dispatched one at a time: the loop waits for
/// `SimEvent::ActionFinished` before sending the next queued action.
fn run_sim_thread(
    construction_plan: ConstructionPlan,
    sim_rx: crossbeam_channel::Receiver<SimCmd>,
    mut speed: SimSpeed,
) {
    // Channels bridging the normal-application thread (this function) and the
    // Bevy app running in the same thread.
    let (action_tx, action_rx) = crossbeam_channel::unbounded::<(Uuid, ConstructionAction)>();
    let (event_tx, event_rx) = crossbeam_channel::unbounded::<SimEvent>();

    let (player_eco, building_queue) = construction_plan.into_parts();

    let mut app = App::new();
    app.add_plugins(EcoPlugin)
        .add_message::<BuildingFinished>()
        // Seed the economy from the plan.
        .insert_resource(PlayerEco(player_eco))
        // One tick represents one simulation second.
        .insert_resource(Time::new(1.0))
        // Bridge resources: actions come in, events go out.
        .insert_resource(ActionReceiver(action_rx))
        .insert_resource(EventSender(event_tx))
        // Forward engine-internal events to the outgoing channel.
        .add_observer(forward_eco_summary)
        .add_systems(
            Update,
            (spawn_incoming_actions, report_finished_constructions),
        );

    let mut run_state = SimRunState::Running;
    let mut queue = building_queue.into_iter().collect::<VecDeque<_>>();
    let mut current_task: Option<Uuid> = None;

    // Seed the first action before the first tick.
    send_next_action(&mut queue, &action_tx, &mut current_task);

    loop {
        if run_state == SimRunState::Stopped {
            break;
        }

        // Drain external commands.
        while let Ok(cmd) = sim_rx.try_recv() {
            match cmd {
                SimCmd::Start => run_state = SimRunState::Running,
                SimCmd::Pause => run_state = SimRunState::Paused,
                SimCmd::Resume => run_state = SimRunState::Running,
                SimCmd::GameSpeed(new_speed) => speed = new_speed,
            }
        }

        // Drain simulation events and forward them to the caller.
        while let Ok(event) = event_rx.try_recv() {
            println!("{}", event);

            if let SimEvent::ActionFinished(finished_task) = event {
                if current_task == Some(finished_task) {
                    current_task = None;
                    send_next_action(&mut queue, &action_tx, &mut current_task);
                }
            }
        }

        if current_task.is_none() && queue.is_empty() {
            // Queue exhausted and nothing is running.
            break;
        }

        if run_state == SimRunState::Running {
            let tick_start = Instant::now();
            app.update();
            throttle_tick(tick_start, &speed);
        } else {
            // Avoid busy-waiting while paused.
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Sleep long enough to honor the requested ticks-per-second rate.
///
/// `Unlimited` returns immediately, letting the simulation run as fast as
/// hardware allows.  For `TicksPerSecond(n)` we sleep for `1.0 / n` seconds
/// minus the time already spent processing the tick.
fn throttle_tick(tick_start: Instant, speed: &SimSpeed) {
    let Some(interval) = speed.tick_interval_seconds() else {
        return;
    };

    let elapsed = tick_start.elapsed();
    let target = Duration::from_secs_f64(interval);
    if let Some(remaining) = target.checked_sub(elapsed) {
        std::thread::sleep(remaining);
    }
}

/// Send the next queued construction action into the Bevy app.
///
/// Does nothing if an action is already in flight.  `run_sim_thread` waits
/// for the matching `SimEvent::ActionFinished` before calling this again.
fn send_next_action(
    queue: &mut VecDeque<ConstructionAction>,
    action_tx: &crossbeam_channel::Sender<(Uuid, ConstructionAction)>,
    current_task: &mut Option<Uuid>,
) {
    if current_task.is_some() {
        return;
    }

    if let Some(action) = queue.pop_front() {
        let task_id = Uuid::new_v4();
        // best-effort send; the receiver lives as long as the app
        let _ = action_tx.send((task_id, action));
        *current_task = Some(task_id);
    }
}
