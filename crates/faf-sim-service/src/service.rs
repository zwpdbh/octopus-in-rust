use crate::simulation::Simulation;
use faf_blueprints::ConstructionPlan;
use faf_sim_protocol::SimCmd;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

pub type SimulationId = Uuid;

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

    pub fn run(&self, construction_plan: ConstructionPlan) {
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
            service.lock().unwrap().remove(&simulation_id)
        });
    }
}

#[derive(Debug, PartialEq)]
enum SimRunState {
    Running,
    Pasused,
    Stopped,
}

fn run_sim_thread(
    construction_plan: ConstructionPlan,
    sim_rx: crossbeam_channel::Receiver<SimCmd>,
) {
    let mut sim = Simulation::new(construction_plan);
    let mut run_state = SimRunState::Running;

    loop {
        // TODO:: run simulation while use commands to control it, like pause, stop or speedup.
    }
}
