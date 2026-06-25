//! Planner actor that observes the simulation and emits commands.
//!
//! The actor receives observations, delegates the decision to the underlying
//! [`Planner`], and forwards the resulting command back to the simulation.

use faf_units::{DataIndex, Unit};
use tokio::sync::mpsc::{Receiver, Sender};

use crate::message::{Command, Observation};
use crate::planner::search::SearchAction;
use crate::planner::Planner;

fn search_action_to_command(action: SearchAction) -> Option<Command> {
    match action {
        SearchAction::Build { unit_id, builders } => Some(Command::Build { unit_id, builders }),
        SearchAction::Assist {
            project_node,
            builders,
        } => Some(Command::Assist {
            project_node,
            builders,
        }),
        SearchAction::Wait => None,
    }
}

/// Actor that runs a planner and exchanges messages with a simulation.
pub struct PlannerActor {
    planner: Planner,
    index: DataIndex,
    goal: Unit,
    obs_rx: Receiver<Observation>,
    cmd_tx: Sender<Command>,
}

impl PlannerActor {
    /// Create a new planner actor.
    pub fn new(
        planner: Planner,
        index: DataIndex,
        goal: Unit,
        obs_rx: Receiver<Observation>,
        cmd_tx: Sender<Command>,
    ) -> Self {
        Self {
            planner,
            index,
            goal,
            obs_rx,
            cmd_tx,
        }
    }

    /// Run the actor until the simulation disconnects.
    ///
    /// State observations are passed to the planner; events are ignored because
    /// they do not trigger a new decision on their own.
    pub async fn run(mut self) {
        while let Some(observation) = self.obs_rx.recv().await {
            let command = match observation {
                Observation::State(state) => {
                    let plan = self.planner.plan(&self.index, state, &self.goal).ok();
                    plan.and_then(|p| p.first_action)
                        .and_then(search_action_to_command)
                }
                // Events alone do not trigger a new decision; wait for the next state snapshot.
                Observation::Event(_) => None,
            };

            if let Some(command) = command {
                if self.cmd_tx.send(command).await.is_err() {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use faf_units::DataIndex;
    use tokio::sync::mpsc;

    use crate::message::{Command, Observation};
    use crate::planner::{Planner, Strategy};
    use crate::planner_actor::PlannerActor;
    use crate::sim_actor::SimActor;

    fn load_index() -> DataIndex {
        let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
        serde_json::from_str(json).expect("embedded index should parse")
    }

    #[tokio::test]
    async fn reactive_beam_actor_reaches_pgen() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let pgen = index.find_unit("URB1101").expect("T1 pgen exists");
        let sim_dt = 0.5;

        let (obs_tx, mut obs_rx) = mpsc::channel::<Observation>(64);
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);

        let sim = SimActor::new(&[acu], index.clone(), Some(pgen), sim_dt, obs_tx, cmd_rx);
        tokio::spawn(sim.run());

        let planner = Planner::with_config(
            Strategy::Beam { beam_width: 20 },
            crate::planner::PlannerConfig {
                dt: 10.0,
                max_depth: 30,
                ..crate::planner::PlannerConfig::default()
            },
        );

        let mut goal_reached = false;
        for _ in 0..1000 {
            match obs_rx.recv().await {
                Some(Observation::State(state)) => {
                    let cmd = planner
                        .plan(&index, state, &pgen)
                        .ok()
                        .and_then(|p| p.first_action)
                        .and_then(super::search_action_to_command);
                    if let Some(cmd) = cmd {
                        if cmd_tx.send(cmd).await.is_err() {
                            break;
                        }
                    }
                }
                Some(Observation::Event(e)) => {
                    if e.unit_id.eq_ignore_ascii_case("URB1101") {
                        goal_reached = true;
                        break;
                    }
                }
                None => break,
            }
        }
        drop(cmd_tx);

        assert!(
            goal_reached,
            "reactive beam planner should build the T1 pgen"
        );
    }

    #[tokio::test]
    async fn reactive_greedy_actor_reaches_pgen() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let pgen = index.find_unit("URB1101").expect("T1 pgen exists");

        let (obs_tx, mut obs_rx) = mpsc::channel::<Observation>(64);
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);

        let sim = SimActor::new(&[acu], index.clone(), Some(pgen), 0.5, obs_tx, cmd_rx);
        tokio::spawn(sim.run());

        let planner = Planner::new(Strategy::Greedy);

        let mut goal_reached = false;
        for _ in 0..1000 {
            match obs_rx.recv().await {
                Some(Observation::State(state)) => {
                    let cmd = planner
                        .plan(&index, state, &pgen)
                        .ok()
                        .and_then(|p| p.first_action)
                        .and_then(super::search_action_to_command);
                    if let Some(cmd) = cmd {
                        if cmd_tx.send(cmd).await.is_err() {
                            break;
                        }
                    }
                }
                Some(Observation::Event(e)) => {
                    if e.unit_id.eq_ignore_ascii_case("URB1101") {
                        goal_reached = true;
                        break;
                    }
                }
                None => break,
            }
        }
        drop(cmd_tx);

        assert!(
            goal_reached,
            "reactive greedy planner should build the T1 pgen"
        );
    }

    #[tokio::test]
    async fn sim_actor_ticks_without_planner_commands() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let dt = 0.1;

        let (obs_tx, mut obs_rx) = mpsc::channel::<Observation>(64);
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);

        let sim = SimActor::new(&[acu], index.clone(), None, dt, obs_tx, cmd_rx);
        let handle = tokio::spawn(sim.run());

        // The simulation ticks on its own timer. Collect observations for a
        // short while without sending any commands, then disconnect.
        let mut observations = 0;
        for _ in 0..20 {
            if obs_rx.recv().await.is_some() {
                observations += 1;
            } else {
                break;
            }
        }
        drop(cmd_tx);

        let final_state = handle.await.unwrap().unwrap();
        assert!(observations > 0);

        // Without a goal the simulation should still contain the starting ACU.
        assert!(
            final_state
                .graph
                .graph
                .node_weights()
                .any(|n| n.is_active() && n.unit_id.eq_ignore_ascii_case("URL0001")),
            "ACU should still be present after ticks"
        );
    }

    #[tokio::test]
    async fn actor_loop_reaches_goal_and_stops() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let pgen = index.find_unit("URB1101").expect("T1 pgen exists");

        let (obs_tx, obs_rx) = mpsc::channel::<Observation>(64);
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);

        let sim = SimActor::new(&[acu], index.clone(), Some(pgen), 0.5, obs_tx, cmd_rx);
        let sim_handle = tokio::spawn(sim.run());

        let planner = Planner::with_config(
            Strategy::Beam { beam_width: 20 },
            crate::planner::PlannerConfig {
                dt: 10.0,
                max_depth: 30,
                ..crate::planner::PlannerConfig::default()
            },
        );
        let planner_actor = PlannerActor::new(planner, index.clone(), pgen.clone(), obs_rx, cmd_tx);
        let planner_handle = tokio::spawn(planner_actor.run());

        // Both actors should shut down cleanly once the pgen is completed.
        let (sim_result, planner_result) = tokio::join!(sim_handle, planner_handle);
        let final_state = sim_result.unwrap().unwrap();
        planner_result.unwrap();

        // The simulation stops after the goal is reached, so the authoritative
        // state must contain the completed pgen.
        assert!(
            final_state
                .graph
                .graph
                .node_weights()
                .any(|n| n.is_active() && n.unit_id.eq_ignore_ascii_case("URB1101")),
            "final state should contain the completed pgen"
        );
    }
}
