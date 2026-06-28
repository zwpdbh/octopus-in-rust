//! Decision actor that observes the simulation and emits commands.
//!
//! The actor receives observations, delegates the decision to the underlying
//! [`Planner`], and forwards the resulting command back to the simulation.

use tokio::sync::mpsc::{Receiver, Sender};

use crate::actors::message::{Command, Observation};
use crate::planner::search::SearchAction;
use crate::planner::Planner;
use crate::units::{UnitKind, Units};

fn search_action_to_command(action: SearchAction) -> Option<Command> {
    match action {
        SearchAction::Build { unit_id, builder } => Some(Command::Build { unit_id, builder }),
        SearchAction::Upgrade {
            target_unit_id,
            old_node,
            builder,
        } => Some(Command::Upgrade {
            target_unit_id,
            old_node,
            builder,
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
pub struct DecisionActor {
    planner: Planner,
    units: Units,
    goal_id: UnitKind,
    obs_rx: Receiver<Observation>,
    cmd_tx: Sender<Command>,
}

impl DecisionActor {
    /// Create a new decision actor.
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

    use crate::actors::message::{Command, Observation};
    use crate::actors::sim_actor::SimActor;
    use crate::units::{UnitKind, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
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

    // Tests that exercise the decision actor with a concrete planner will be
    // re-added once the MCTS planner is implemented.
}
