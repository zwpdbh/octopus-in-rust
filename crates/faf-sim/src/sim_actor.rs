//! Simulation actor that runs the graph-growth model and exchanges messages
//! with a planner actor.
//!
//! The actor owns the authoritative [`GraphState`] and advances it in fixed
//! ticks. It is decoupled from the planner: it keeps ticking even when no
//! command arrives, which lets it simulate an AFK player or a slow planner.
//!
//! Communication model:
//!
//! - The actor receives external commands from the planner on `cmd_rx`.
//! - An internal timer fires every `dt` seconds to advance the simulation.
//! - After each tick, if any build event occurred, the actor sends an
//!   observation to the planner summarizing the new state.

use std::time::Duration;

use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::{interval, Interval};

use crate::message::{Command, Observation};
use crate::sim::{BuildEvent, GraphSimError, GraphState, NodeId};
use crate::units::Units;

/// Actor that runs the simulation.
pub struct SimActor {
    /// Authoritative simulation state.
    pub state: GraphState,
    /// Unified unit knowledge repository.
    pub units: Units,
    /// Goal unit id that, when completed, stops the simulation.
    pub goal: Option<String>,
    /// Fixed tick duration in seconds.
    pub dt: f64,
    /// Timer that drives the simulation tick loop.
    pub timer: Interval,
    /// Commands queued from the planner but not yet applied.
    pub command_queue: Vec<Command>,
    /// Channel on which to send observations to the planner.
    pub obs_tx: Sender<Observation>,
    /// Channel on which to receive commands from the planner.
    pub cmd_rx: Receiver<Command>,
}

impl SimActor {
    /// Create a new simulation actor starting from `starting_units`.
    ///
    /// If `goal` is `Some`, the actor will stop automatically once that unit has
    /// been completed.
    pub fn new(
        starting_unit_ids: &[&str],
        units: Units,
        goal_id: Option<&str>,
        dt: f64,
        obs_tx: Sender<Observation>,
        cmd_rx: Receiver<Command>,
    ) -> Self {
        let state = GraphState::new(&units, starting_unit_ids);
        let timer = interval(Duration::from_secs_f64(dt));
        Self {
            state,
            units,
            goal: goal_id.map(|id| id.to_string()),
            dt,
            timer,
            command_queue: Vec::new(),
            obs_tx,
            cmd_rx,
        }
    }

    /// Run the actor until the goal is reached, the planner disconnects, or a
    /// fatal error occurs.
    ///
    /// On success the final authoritative [`GraphState`] is returned, including
    /// the completion timeline and final economy.
    pub async fn run(mut self) -> Result<GraphState, GraphSimError> {
        loop {
            tokio::select! {
                // Internal timer: advance the simulation regardless of whether
                // the planner has sent a command. This lets us simulate an AFK
                // player.
                _ = self.timer.tick() => {
                    self.tick_and_report().await?;
                    if self.goal_reached() {
                        break;
                    }
                }

                // External command: queue it for application on the next tick.
                maybe_cmd = self.cmd_rx.recv() => {
                    match maybe_cmd {
                        Some(cmd) => self.command_queue.push(cmd),
                        None => break,
                    }
                }
            }
        }

        Ok(self.state)
    }

    /// Apply any queued commands, advance one tick, and report events.
    async fn tick_and_report(&mut self) -> Result<(), GraphSimError> {
        // Apply all queued commands before the tick.
        let commands: Vec<Command> = self.command_queue.drain(..).collect();
        for command in commands {
            self.apply_command(command)?;
        }

        let completed = self.state.tick(&self.units, self.dt);

        // Report each completed unit as an event.
        for node_id in completed {
            let event = self.build_event(node_id);
            if self.obs_tx.send(Observation::Event(event)).await.is_err() {
                break;
            }
        }

        // Send a summarized state observation after every tick so the planner
        // can react to idle builders, economy changes, etc.
        if self
            .obs_tx
            .send(Observation::State(self.state.clone()))
            .await
            .is_err()
        {
            // Planner disconnected; the outer loop will exit.
        }

        Ok(())
    }

    /// Returns true if the configured goal unit has been completed.
    fn goal_reached(&self) -> bool {
        let Some(goal_id) = &self.goal else {
            return false;
        };
        self.state.goal_reached(goal_id)
    }

    /// Apply a planner command to the simulation state.
    fn apply_command(&mut self, command: Command) -> Result<(), GraphSimError> {
        match command {
            Command::Build { unit_id, builders } => {
                self.state.start_project(&unit_id, &builders, &self.units)?;
            }
            Command::Assist {
                project_node,
                builders,
            } => {
                if builders.is_empty() {
                    return Ok(());
                }
                let project_index = self
                    .state
                    .active_projects
                    .iter()
                    .position(|p| p.target_node == project_node)
                    .ok_or(GraphSimError::ProjectNotFound)?;
                self.state
                    .assist_project(project_index, &builders, &self.units)?;
            }
            Command::Upgrade {
                target_unit_id,
                old_node,
                builders,
            } => {
                self.state
                    .start_upgrade_project(&target_unit_id, old_node, &builders, &self.units)?;
            }
        }
        Ok(())
    }

    /// Build a `BuildEvent` for a completed node.
    fn build_event(&self, node_id: NodeId) -> BuildEvent {
        let unit_id = self.state.graph[node_id].unit_id.clone();
        let unit_name = self
            .units
            .find(&unit_id)
            .map(|u| u.display_name().to_string())
            .unwrap_or_else(|| unit_id.clone());
        BuildEvent {
            time: self.state.time,
            unit_id,
            unit_name,
        }
    }
}
