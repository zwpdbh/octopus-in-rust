//! Coordinator that pairs unit/build-order state with economy/clock state.
//!
//! [`Simulation`] owns a [`UnitGraph`] and an [`EcoEngine`] and provides a single
//! `tick` call that advances both. It is intended as the higher-level state
//! object that replaces the legacy `SimulationState` adapter.
//! once all callers have been migrated.

use std::ops::{Deref, DerefMut};

use crate::economy::EconomyState;
use crate::engine::unit_graph::{BuildEvent, UnitGraph};
use crate::engine::EcoEngine;
use crate::units::{UnitKind, Units};

/// Combined unit/build-order and economy/clock state.
#[derive(Debug, Clone)]
pub struct Simulation {
    /// Unit/build-order state.
    pub graph: UnitGraph,
    /// Economy state and simulation clock.
    pub engine: EcoEngine,
}

impl Simulation {
    /// Create a new simulation starting from the given units.
    ///
    /// The initial economy is derived from the starting units. The simulation
    /// starts at tick 0 with no command delay.
    pub fn new(starting_units: &[UnitKind], units: Units, ticks_per_second: u64) -> Self {
        Self::with_delay(starting_units, units, ticks_per_second, 0.0)
    }

    /// Create a new simulation with a specific command delay.
    pub fn with_delay(
        starting_units: &[UnitKind],
        units: Units,
        ticks_per_second: u64,
        command_delay_seconds: f64,
    ) -> Self {
        let graph = UnitGraph::new(starting_units, units);
        let economy = graph.derive_economy();
        let engine = EcoEngine::with_delay(economy, ticks_per_second, command_delay_seconds);
        Self { graph, engine }
    }

    /// Current simulation time in seconds.
    pub fn time(&self) -> f64 {
        self.engine.time_seconds()
    }

    /// Current economy state.
    pub fn economy(&self) -> &EconomyState {
        &self.engine.economy
    }

    /// Borrow the build events collected so far.
    pub fn events(&self) -> &[BuildEvent] {
        &self.graph.events
    }

    /// Advance the simulation by `dt` seconds.
    ///
    /// Runs the unit-graph tick against the engine's economy, rebuilds the
    /// economy if any units completed, and advances the engine's tick counter.
    /// Returns the build events that fired during the tick.
    pub fn tick(&mut self, dt: f64) -> Vec<BuildEvent> {
        if dt <= 0.0 {
            return Vec::new();
        }

        let completed = self.graph.tick(&mut self.engine.economy, dt);
        if !completed.is_empty() {
            self.engine.economy = self.graph.derive_economy();
        }

        let ticks = EcoEngine::seconds_to_ticks(self.engine.ticks_per_second, dt);
        self.engine.tick = self.engine.tick.advance(ticks);

        completed
            .into_iter()
            .map(|node_id| {
                let unit_id = self.graph.graph[node_id].unit_id.clone();
                let unit_name = self.graph.units.display_name(&unit_id);
                let finish_time = match &self.graph.graph[node_id].state {
                    crate::engine::unit_graph::UnitNodeState::Constructed {
                        finish_time, ..
                    } => *finish_time,
                    crate::engine::unit_graph::UnitNodeState::Upgraded { finish_time, .. } => {
                        *finish_time
                    }
                    _ => self.graph.time,
                };
                BuildEvent {
                    time: finish_time,
                    unit_id,
                    unit_name,
                    node_id,
                }
            })
            .collect()
    }
}

impl Deref for Simulation {
    type Target = UnitGraph;

    fn deref(&self) -> &Self::Target {
        &self.graph
    }
}

impl DerefMut for Simulation {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.graph
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::unit_command::{UnitAction, UnitCommand};
    use crate::engine::unit_graph::NodeId;
    use crate::units::TechLevel;

    fn load_units() -> Units {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn simulation_builds_t1_mex() {
        let units = load_units();
        let mut sim = Simulation::new(&[UnitKind::Commander], units, 1);

        sim.graph
            .start_project(&UnitKind::Mex(TechLevel::T1), &[NodeId::new(0)])
            .expect("ACU can build mex");

        for _ in 0..60 {
            if !sim.tick(1.0).is_empty() {
                break;
            }
        }

        assert!(
            sim.graph
                .events
                .iter()
                .any(|e| e.unit_id == UnitKind::Mex(TechLevel::T1)),
            "expected T1 mex completion event"
        );
        assert!(sim.economy().net_mass_income.value() > 1.0);
    }

    #[test]
    fn simulation_rebuilds_economy_after_completion() {
        let units = load_units();
        let mut sim = Simulation::new(&[UnitKind::Commander], units, 1);

        let initial_income = sim.economy().net_mass_income.value();

        sim.graph
            .start_project(&UnitKind::Mex(TechLevel::T1), &[NodeId::new(0)])
            .expect("ACU can build mex");

        for _ in 0..60 {
            sim.tick(1.0);
            if sim
                .graph
                .events
                .iter()
                .any(|e| e.unit_id == UnitKind::Mex(TechLevel::T1))
            {
                break;
            }
        }

        assert!(sim.economy().net_mass_income.value() > initial_income);
    }

    #[test]
    fn simulation_advances_tick() {
        let units = load_units();
        let mut sim = Simulation::new(&[UnitKind::Commander], units, 10);

        assert_eq!(sim.engine.tick.0, 0);
        sim.tick(0.5);
        assert_eq!(sim.engine.tick.0, 5);
    }
}
