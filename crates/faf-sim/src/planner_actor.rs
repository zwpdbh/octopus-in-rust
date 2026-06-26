//! Planner actor that observes the simulation and emits commands.
//!
//! The actor receives observations, delegates the decision to the underlying
//! [`Planner`], and forwards the resulting command back to the simulation.

use tokio::sync::mpsc::{Receiver, Sender};

use crate::message::{Command, Observation};
use crate::planner::search::SearchAction;
use crate::planner::Planner;
use crate::units::{UnitKind, Units};

fn search_action_to_command(action: SearchAction) -> Option<Command> {
    match action {
        SearchAction::Build { unit_id, builders } => Some(Command::Build { unit_id, builders }),
        SearchAction::Upgrade {
            target_unit_id,
            old_node,
            builders,
        } => Some(Command::Upgrade {
            target_unit_id,
            old_node,
            builders,
        }),
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
    units: Units,
    goal_id: UnitKind,
    obs_rx: Receiver<Observation>,
    cmd_tx: Sender<Command>,
}

impl PlannerActor {
    /// Create a new planner actor.
    pub fn new(
        planner: Planner,
        units: Units,
        goal_id: UnitKind,
        obs_rx: Receiver<Observation>,
        cmd_tx: Sender<Command>,
    ) -> Self {
        Self {
            planner,
            units,
            goal_id,
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
                    let plan = self.planner.plan(&self.units, state, &self.goal_id).ok();
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
    use tokio::sync::mpsc;

    use crate::message::{Command, Observation};
    use crate::planner::{Planner, Strategy};
    use crate::planner_actor::PlannerActor;
    use crate::sim_actor::SimActor;
    use crate::units::{TechLevel, UnitKind, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[tokio::test]
    async fn reactive_beam_actor_reaches_pgen() {
        let units = load_units();
        let sim_dt = 0.5;

        let (obs_tx, mut obs_rx) = mpsc::channel::<Observation>(64);
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);

        let sim = SimActor::new(
            &[UnitKind::Commander],
            units.clone(),
            Some(UnitKind::Pgen(TechLevel::T1)),
            sim_dt,
            obs_tx,
            cmd_rx,
        );
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
                        .plan(&units, state, &UnitKind::Pgen(TechLevel::T1))
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
                    if e.unit_id == UnitKind::Pgen(TechLevel::T1) {
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
        let units = load_units();

        let (obs_tx, mut obs_rx) = mpsc::channel::<Observation>(64);
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);

        let sim = SimActor::new(
            &[UnitKind::Commander],
            units.clone(),
            Some(UnitKind::Pgen(TechLevel::T1)),
            0.5,
            obs_tx,
            cmd_rx,
        );
        tokio::spawn(sim.run());

        let planner = Planner::new(Strategy::Greedy);

        let mut goal_reached = false;
        for _ in 0..1000 {
            match obs_rx.recv().await {
                Some(Observation::State(state)) => {
                    let cmd = planner
                        .plan(&units, state, &UnitKind::Pgen(TechLevel::T1))
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
                    if e.unit_id == UnitKind::Pgen(TechLevel::T1) {
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
        let units = load_units();
        let dt = 0.1;

        let (obs_tx, mut obs_rx) = mpsc::channel::<Observation>(64);
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);

        let sim = SimActor::new(
            &[UnitKind::Commander],
            units.clone(),
            None,
            dt,
            obs_tx,
            cmd_rx,
        );
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
            final_state.has_completed_unit(&UnitKind::Commander),
            "ACU should still be present after ticks"
        );
    }

    #[tokio::test]
    async fn actor_loop_reaches_goal_and_stops() {
        let units = load_units();

        let (obs_tx, obs_rx) = mpsc::channel::<Observation>(64);
        let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);

        let sim = SimActor::new(
            &[UnitKind::Commander],
            units.clone(),
            Some(UnitKind::Pgen(TechLevel::T1)),
            0.5,
            obs_tx,
            cmd_rx,
        );
        let sim_handle = tokio::spawn(sim.run());

        let planner = Planner::with_config(
            Strategy::Beam { beam_width: 20 },
            crate::planner::PlannerConfig {
                dt: 10.0,
                max_depth: 30,
                ..crate::planner::PlannerConfig::default()
            },
        );
        let planner_actor = PlannerActor::new(
            planner,
            units.clone(),
            UnitKind::Pgen(TechLevel::T1),
            obs_rx,
            cmd_tx,
        );
        let planner_handle = tokio::spawn(planner_actor.run());

        // Both actors should shut down cleanly once the pgen is completed.
        let (sim_result, planner_result) = tokio::join!(sim_handle, planner_handle);
        let final_state = sim_result.unwrap().unwrap();
        planner_result.unwrap();

        // The simulation stops after the goal is reached, so the authoritative
        // state must contain the completed pgen.
        assert!(
            final_state.has_completed_unit(&UnitKind::Pgen(TechLevel::T1)),
            "final state should contain the completed pgen"
        );
    }
}
