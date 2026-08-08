use crate::sim_systems::*;
use bevy_app::prelude::*;
use bevy_ecs::schedule::IntoScheduleConfigs;
use faf_blueprints::{ConstructionAction, ConstructionPlan};
use faf_game_engine::*;
use faf_sim_protocol::{SimCmd, SimEvent, SimSpeed};
use std::{
    collections::VecDeque,
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

/// Handle returned when a simulation is started.
///
/// Callers can send runtime commands through `cmd_tx` and receive simulation
/// events through `event_rx`.  The channel closes when the simulation thread
/// exits (queue exhausted or stopped).
#[derive(Debug)]
pub struct SimulationController {
    pub id: SimulationId,
    pub cmd_tx: crossbeam_channel::Sender<SimCmd>,
    pub event_rx: crossbeam_channel::Receiver<SimEvent>,
}

/// Launcher for Bevy-backed construction simulations.
#[derive(Debug, Default)]
pub struct SimulationService;

impl SimulationService {
    pub fn new() -> Self {
        Self
    }

    /// Start a simulation in a background thread at the given speed.
    ///
    /// Returns a [`SimulationController`] that can be used to send commands
    /// and read events.
    pub fn run(
        &self,
        construction_plan: ConstructionPlan,
        speed: SimSpeed,
    ) -> SimulationController {
        let simulation_id = Uuid::new_v4();
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<SimCmd>();
        let (event_tx, event_rx) = crossbeam_channel::unbounded::<SimEvent>();

        std::thread::spawn(move || {
            run_sim_thread(construction_plan, cmd_rx, event_tx, speed);
        });

        SimulationController {
            id: simulation_id,
            cmd_tx,
            event_rx,
        }
    }

    /// Start a simulation and block until it completes.
    pub fn run_blocking(
        &self,
        construction_plan: ConstructionPlan,
        speed: SimSpeed,
    ) -> Result<(), SimulationError> {
        let controller = self.run(construction_plan, speed);
        let handle = spawn_controller_waiter(controller);
        handle.join().map_err(|_| SimulationError::ThreadPanicked)?;
        Ok(())
    }
}

fn spawn_controller_waiter(controller: SimulationController) -> JoinHandle<()> {
    std::thread::spawn(move || {
        while let Ok(event) = controller.event_rx.recv() {
            println!("{}", event);
        }
    })
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
/// 2. Checks for finished construction tasks and dispatches the next queued
///    action if the previous one completed.
/// 3. Steps the engine by one tick (`app.update()`).
/// 4. Throttles the loop to match the requested `SimSpeed`.
///
/// Actions are dispatched one at a time: the loop waits for an internal
/// completion signal before sending the next queued action.
fn run_sim_thread(
    construction_plan: ConstructionPlan,
    cmd_rx: crossbeam_channel::Receiver<SimCmd>,
    event_tx: crossbeam_channel::Sender<SimEvent>,
    mut speed: SimSpeed,
) {
    // Channel from service thread into the Bevy app for incoming actions.
    let (action_tx, action_rx) = crossbeam_channel::unbounded::<(Uuid, ConstructionAction)>();
    // Internal channel from the Bevy app back to the service thread for
    // task-completion notifications.
    let (finished_tx, finished_rx) = crossbeam_channel::unbounded::<Uuid>();

    let (player_eco, building_queue) = construction_plan.into_parts();

    let mut app = App::new();
    app.add_plugins(EcoPlugin)
        .add_message::<BuildingFinished>()
        // Seed the economy from the plan.
        .insert_resource(PlayerEco(player_eco))
        // One tick represents one simulation second.
        .insert_resource(Time::new(1.0))
        // Bridge resources: actions come in, events go out, completions are
        // tracked internally.
        .insert_resource(ActionReceiver(action_rx))
        .insert_resource(EventSender(event_tx))
        .insert_resource(FinishedSender(finished_tx))
        .add_systems(
            Update,
            (
                spawn_incoming_actions
                    .before(player_eco_systems::update_player_eco_from_building_units),
                report_finished_constructions
                    .after(player_eco_systems::apply_finished_constructions),
                emit_eco_summary.after(player_eco_systems::apply_finished_constructions),
            ),
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
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                SimCmd::Start => run_state = SimRunState::Running,
                SimCmd::Pause => run_state = SimRunState::Paused,
                SimCmd::Resume => run_state = SimRunState::Running,
                SimCmd::Stop => {
                    run_state = SimRunState::Stopped;
                }
                SimCmd::GameSpeed(new_speed) => {
                    // Speed only affects wall-clock cadence via throttle_tick;
                    // the engine itself stays tick-based.
                    speed = new_speed;
                }
            }
        }

        // If the current action finished, dispatch the next one.
        while let Ok(finished_task) = finished_rx.try_recv() {
            if current_task == Some(finished_task) {
                current_task = None;
                send_next_action(&mut queue, &action_tx, &mut current_task);
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
/// for the matching internal completion signal before calling this again.
/// Because actions are dispatched between `app.update()` ticks, each action
/// naturally starts one simulation second after the previous one finishes,
/// giving the default 1-second in-game delay required by the UI.
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
