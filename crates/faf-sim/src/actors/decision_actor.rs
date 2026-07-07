//! Decision actor that observes the simulation and emits commands.
//!
//! The actor receives observations, delegates the decision to the underlying
//! [`Planner`], and forwards the resulting command back to the simulation.

use tokio::sync::mpsc::{Receiver, Sender};

use crate::actors::message::{Observation, SimulationMsg};
use crate::planner::core::Goal;
use crate::planner::Planner;
use crate::planner::SimAction;
use crate::units::Units;

fn sim_action_to_command(action: SimAction) -> Option<SimulationMsg> {
    match action {
        SimAction::Build { unit_id, builders } => Some(SimulationMsg::Build { unit_id, builders }),
        SimAction::Upgrade {
            target_unit_id,
            old_node,
            builders,
        } => Some(SimulationMsg::Upgrade {
            target_unit_id,
            old_node,
            builders,
        }),
        SimAction::Assist {
            project_node,
            builders,
        } => Some(SimulationMsg::Assist {
            project_node,
            builders,
        }),
        SimAction::BuildGoal { goal, builders } => {
            Some(SimulationMsg::BuildGoal { goal, builders })
        }
        SimAction::Wait => None,
    }
}

/// Actor that runs a planner and exchanges messages with a simulation.
pub struct DecisionActor {
    planner: Planner,
    units: Units,
    goal: Goal,
    obs_rx: Receiver<Observation>,
    cmd_tx: Sender<SimulationMsg>,
}

impl DecisionActor {
    /// Create a new decision actor.
    pub fn new(
        planner: Planner,
        units: Units,
        goal: Goal,
        obs_rx: Receiver<Observation>,
        cmd_tx: Sender<SimulationMsg>,
    ) -> Self {
        Self {
            planner,
            units,
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
            match observation {
                Observation::State(state) => {
                    // The reactive loop commits to a single action per simulator
                    // tick. This matches the training-time greedy evaluator where
                    // the policy evaluates once, executes the chosen direction,
                    // and then advances the simulation by `dt` before deciding
                    // again.
                    let plan = match self.planner.plan(&self.units, state, &self.goal) {
                        Ok(p) => p,
                        Err(_) => continue,
                    };

                    let Some(command) = plan.first_action.and_then(sim_action_to_command) else {
                        continue;
                    };

                    if self.cmd_tx.send(command).await.is_err() {
                        return;
                    }
                }
                // Events alone do not trigger a new decision; wait for the next state snapshot.
                Observation::Event(_) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;

    use crate::actors::message::{Observation, SimulationMsg};
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
        let (cmd_tx, cmd_rx) = mpsc::channel::<SimulationMsg>(64);

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
    // re-added once the policy planner is fully validated.
}
