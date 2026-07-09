//! Coordinator that pairs unit/build-order state with economy/clock state.
//!
//! [`Simulation`] owns a [`UnitGraph`] and an [`EcoEngine`] and provides a single
//! `tick` call that advances both. It is intended as the higher-level state
//! object that replaces the legacy `SimulationState` adapter.
//!
//! `Simulation` is message-driven: it creates [`Construction`] actors in the
//! unit graph, tells [`EcoEngine`] about them, feeds current build powers into
//! each tick, and routes completion events back to the unit graph.

use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use crate::economy::EconomyState;
use crate::engine::unit_graph::{BuildEvent, GraphSimError, NodeId, UnitGraph};
use crate::engine::{ConstructionId, EcoCommand, EcoEngine, EcoEvent};
use crate::planner::core::Goal;
use crate::units::{UnitKind, Units};

/// Combined unit/build-order and economy/clock state.
///
/// `Simulation` acts as a message router between three actors:
///
/// - [`UnitGraph`] owns the graph of units and living [`Construction`] actors.
///   It only mutates when it receives a build/upgrade/assist command or a
///   completion event.
/// - [`EcoEngine`] owns the economy state and clock. It knows each construction
///   only as an id, a build power, and a cost, and it computes mass/energy
///   drain internally.
/// - [`Construction`] actors bridge the two: the unit graph creates them, the
///   eco engine tracks their progress, and `Simulation` maps completion events
///   back to graph nodes.
#[derive(Debug, Clone)]
pub struct Simulation {
    /// Unit/build-order state.
    pub graph: UnitGraph,
    /// Economy state and simulation clock.
    pub engine: EcoEngine,
    /// Next id to assign to a construction in the eco engine.
    next_construction_id: usize,
    /// Map from eco-engine construction id to the graph node it represents.
    construction_to_node: HashMap<ConstructionId, NodeId>,
    /// Map from graph node to the eco-engine construction id.
    node_to_construction: HashMap<NodeId, ConstructionId>,
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
        Self {
            graph,
            engine,
            next_construction_id: 1,
            construction_to_node: HashMap::new(),
            node_to_construction: HashMap::new(),
        }
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

    /// Start a new unit construction and register it with the economy engine.
    pub fn start_project(
        &mut self,
        unit: &UnitKind,
        builders: &[NodeId],
    ) -> Result<NodeId, GraphSimError> {
        let node_id = self.graph.start_project(unit, builders)?;

        let cost = self
            .graph
            .units
            .build_cost(unit)
            .expect("node just created from a known unit");
        let power = self.graph.project_build_power(node_id);
        let construction_id = self.alloc_construction_id();

        self.graph.add_construction(construction_id, node_id);
        self.engine.apply_command(EcoCommand::StartConstruction {
            id: construction_id,
            power,
            cost: cost.to_target_stats(),
        });
        self.construction_to_node.insert(construction_id, node_id);
        self.node_to_construction.insert(node_id, construction_id);

        Ok(node_id)
    }

    /// Start a new upgrade construction and register it with the economy engine.
    pub fn start_upgrade_project(
        &mut self,
        target: &UnitKind,
        old_node: NodeId,
        builders: &[NodeId],
    ) -> Result<NodeId, GraphSimError> {
        let project_node_id = self
            .graph
            .start_upgrade_project(target, old_node, builders)?;

        let cost = {
            let old_kind = &self.graph.graph[old_node].unit_id;
            let recipe = self
                .graph
                .units
                .upgrade_recipes(old_kind)
                .iter()
                .find(|r| r.to == *target)
                .expect("validated upgrade recipe should exist");
            recipe.cost.to_target_stats()
        };
        let power = self.graph.project_build_power(project_node_id);
        let construction_id = self.alloc_construction_id();

        self.graph.add_construction(construction_id, project_node_id);
        self.engine.apply_command(EcoCommand::StartConstruction {
            id: construction_id,
            power,
            cost,
        });
        self.construction_to_node
            .insert(construction_id, project_node_id);
        self.node_to_construction
            .insert(project_node_id, construction_id);

        Ok(project_node_id)
    }

    /// Start the abstract goal construction and register it with the economy
    /// engine.
    pub fn start_goal_project(
        &mut self,
        goal: Goal,
        builders: &[NodeId],
    ) -> Result<(), GraphSimError> {
        self.graph.start_goal_project(goal.clone(), builders)?;
        let power = self.graph.goal_project_build_power();
        self.engine.apply_command(EcoCommand::StartConstruction {
            id: ConstructionId::GOAL,
            power,
            cost: goal.cost().to_target_stats(),
        });
        Ok(())
    }

    /// Assign additional builders to an existing project.
    pub fn assist_project(
        &mut self,
        target_node: NodeId,
        builders: &[NodeId],
    ) -> Result<(), GraphSimError> {
        self.graph.assist_project(target_node, builders)
    }

    /// Advance the simulation by `dt` seconds.
    ///
    /// Collects current build powers from the unit graph, advances the eco
    /// engine, and routes completion events back to the unit graph.
    /// Returns the build events that fired during this tick.
    pub fn tick(&mut self, dt: f64) -> Vec<BuildEvent> {
        if dt <= 0.0 {
            return Vec::new();
        }

        // 1. Collect per-construction build powers from the unit graph.
        let mut powers = Vec::with_capacity(self.node_to_construction.len() + 1);
        for (&node_id, &construction_id) in &self.node_to_construction {
            let power = self.graph.project_build_power(node_id);
            if power > 0.0 {
                powers.push((construction_id, power));
            }
        }
        if self.graph.goal_project_active() {
            let power = self.graph.goal_project_build_power();
            if power > 0.0 {
                powers.push((ConstructionId::GOAL, power));
            }
        }

        let old_event_count = self.graph.events.len();

        // 2. Advance the economy engine.
        let result = self.engine.tick(dt, &powers);
        self.graph.time = self.engine.time_seconds();

        // 3. Route completion events back to the unit graph.
        let mut node_completions = Vec::new();
        let mut goal_completed = false;
        for event in result.events {
            match event {
                EcoEvent::ConstructionCompleted { id, finish_time } => {
                    if id == ConstructionId::GOAL {
                        goal_completed = true;
                    } else if let Some(&node_id) = self.construction_to_node.get(&id) {
                        node_completions.push((node_id, finish_time));
                    }
                }
            }
        }

        if goal_completed {
            self.graph.complete_goal_project();
        }
        if !node_completions.is_empty() {
            self.graph.apply_completions(&node_completions);
            self.engine.economy = self.graph.derive_economy();
            for (node_id, _) in &node_completions {
                if let Some(construction_id) = self.node_to_construction.remove(node_id) {
                    self.construction_to_node.remove(&construction_id);
                    self.graph.remove_construction(construction_id);
                }
            }
        }

        // 4. Return the events emitted during this tick.
        self.graph.events[old_event_count..].to_vec()
    }

    fn alloc_construction_id(&mut self) -> ConstructionId {
        let id = ConstructionId(self.next_construction_id);
        self.next_construction_id += 1;
        id
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

        sim.start_project(&UnitKind::Mex(TechLevel::T1), &[NodeId::new(0)])
            .expect("ACU can build mex");

        for _ in 0..60 {
            if !sim.tick(1.0).is_empty() {
                break;
            }
        }

        assert!(
            sim.events()
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

        sim.start_project(&UnitKind::Mex(TechLevel::T1), &[NodeId::new(0)])
            .expect("ACU can build mex");

        for _ in 0..60 {
            sim.tick(1.0);
            if sim
                .events()
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
