use crate::sim_systems::*;
use bevy_app::prelude::*;
use faf_blueprints::{ConstructionAction, ConstructionPlan};
use faf_game_engine::*;
use faf_sim_protocol::{SimCmd, SimEvent};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    thread::JoinHandle,
};
use uuid::Uuid;

pub type SimulationId = Uuid;

#[derive(Debug, thiserror::Error)]
pub enum SimulationError {
    #[error("simulation thread panicked")]
    ThreadPanicked,
}

#[derive(Debug)]
struct SimulationHandle {
    sim_tx: crossbeam_channel::Sender<SimCmd>,
}

#[derive(Debug, Default)]
pub struct SimulationService {
    simulations: Arc<Mutex<HashMap<SimulationId, SimulationHandle>>>,
}

impl SimulationService {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a simulation in a background thread.
    pub fn run(&self, construction_plan: ConstructionPlan) {
        let _ = self.spawn_run(construction_plan);
    }

    /// Start a simulation and block until it completes.
    pub fn run_blocking(&self, construction_plan: ConstructionPlan) -> Result<(), SimulationError> {
        let handle = self.spawn_run(construction_plan);
        handle.join().map_err(|_| SimulationError::ThreadPanicked)?;
        Ok(())
    }

    fn spawn_run(&self, construction_plan: ConstructionPlan) -> JoinHandle<()> {
        let simulation_id = Uuid::new_v4();
        let (sim_tx, sim_rx) = crossbeam_channel::unbounded::<SimCmd>();

        let sim_handle = SimulationHandle { sim_tx };
        let service = self.simulations.clone();

        self.simulations
            .lock()
            .unwrap()
            .insert(simulation_id, sim_handle);

        std::thread::spawn(move || {
            run_sim_thread(construction_plan, sim_rx);
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

fn run_sim_thread(
    construction_plan: ConstructionPlan,
    sim_rx: crossbeam_channel::Receiver<SimCmd>,
) {
    let (action_tx, action_rx) = crossbeam_channel::unbounded::<(Uuid, ConstructionAction)>();
    let (event_tx, event_rx) = crossbeam_channel::unbounded::<SimEvent>();

    let (player_eco, building_queue) = construction_plan.into_parts();

    let mut app = App::new();
    app.add_plugins(EcoPlugin)
        .add_message::<BuildingFinished>()
        .insert_resource(PlayerEco(player_eco))
        .insert_resource(ActionReceiver(action_rx))
        .insert_resource(EventSender(event_tx))
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
                SimCmd::GameSpeed(_speed) => {
                    // Reserved for future delta-time scaling.
                }
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
            app.update();
        }
    }
}

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
