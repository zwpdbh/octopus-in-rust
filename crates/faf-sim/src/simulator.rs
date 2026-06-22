//! Discrete-time build simulator.
//!
//! The simulator runs concurrent projects (e.g. building engineers while they
//! assist the main target) under any policy that implements `BuildPolicy`.
//! It is deliberately separate from any specific planning strategy.

use faf_units::{DataIndex, Unit};

use crate::economy::{total_build_power, BuildProject, EconomyState, RequestedBuildPower};
use crate::sim::{derive_economy, BuildEvent};
use crate::tech_graph::TechGraph;

/// Priority of an active project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProjectPriority {
    /// The unit we are ultimately trying to finish.
    Goal,
    /// A builder or factory whose purpose is to increase total build power.
    Builder,
    /// A mass extractor, power generator, or other economy structure.
    Economy,
}

/// A project currently being built by the simulator.
#[derive(Debug, Clone)]
pub struct ActiveProject {
    /// Blueprint id of the unit being built.
    pub target_id: String,
    /// Build power this project has been allocated.
    pub requested_bp: RequestedBuildPower,
    /// Remaining-work tracker. This owns a clone of the target unit.
    pub project: BuildProject,
    /// Why this project was started.
    pub priority: ProjectPriority,
}

impl ActiveProject {
    pub(crate) fn new(
        target: &Unit,
        requested_bp: RequestedBuildPower,
        priority: ProjectPriority,
    ) -> Option<Self> {
        let mut project = BuildProject::new(target)?;
        project.assigned_build_power = requested_bp;
        Some(Self {
            target_id: target.id.clone(),
            requested_bp,
            project,
            priority,
        })
    }
}

/// A request from a policy to start a new project.
#[derive(Debug, Clone)]
pub struct ProjectRequest {
    /// Blueprint id of the unit to build.
    pub target_id: String,
    /// Desired build power for this project.
    pub requested_bp: RequestedBuildPower,
    /// Priority category.
    pub priority: ProjectPriority,
}

/// Policy that decides which projects to start.
pub trait BuildPolicy {
    /// Return a list of new projects to start on this tick.
    fn choose_projects<'a>(
        &self,
        graph: &'a TechGraph<'a>,
        state: &EconomyState,
        owned: &[&'a Unit],
        active: &[ActiveProject],
        goal: &'a Unit,
    ) -> Vec<ProjectRequest>;
}

/// Discrete-time simulator that runs concurrent projects under a policy.
pub struct HeuristicSimulator<'a, P: BuildPolicy> {
    /// Unit database.
    pub index: &'a DataIndex,
    /// Dependency graph.
    pub graph: TechGraph<'a>,
    /// Units currently owned.
    pub owned_units: Vec<&'a Unit>,
    /// Current economy state.
    pub state: EconomyState,
    /// Current simulation time in seconds.
    pub current_time: f64,
    /// Fixed timestep in seconds.
    pub dt: f64,
    /// Projects currently under construction.
    pub active_projects: Vec<ActiveProject>,
    /// Completed project events.
    pub events: Vec<BuildEvent>,
    /// Goal unit.
    pub goal: &'a Unit,
    /// Decision policy.
    pub policy: P,
}

impl<'a, P: BuildPolicy> HeuristicSimulator<'a, P> {
    /// Create a new simulator.
    pub fn new(
        index: &'a DataIndex,
        starting_units: Vec<&'a Unit>,
        goal: &'a Unit,
        policy: P,
        dt: f64,
    ) -> Self {
        let state = derive_economy(&starting_units);
        Self {
            index,
            graph: TechGraph::new(index),
            owned_units: starting_units,
            state,
            current_time: 0.0,
            dt,
            active_projects: Vec::new(),
            events: Vec::new(),
            goal,
            policy,
        }
    }

    /// Total build power currently available from owned units.
    pub fn available_bp(&self) -> RequestedBuildPower {
        total_build_power(&self.owned_units)
    }

    /// Run the simulation until the goal unit completes.
    ///
    /// Returns the completion event and the full event history.
    pub fn run(&mut self) -> Option<BuildEvent> {
        let mut goal_event: Option<BuildEvent> = None;
        let mut safety = 0;

        while goal_event.is_none() && safety < 10_000_000 {
            safety += 1;
            self.tick();

            if let Some(event) = self.events.last() {
                if event.unit_id.eq_ignore_ascii_case(&self.goal.id) {
                    goal_event = Some(event.clone());
                }
            }
        }

        goal_event
    }

    /// Advance the simulation by one tick.
    pub fn tick(&mut self) {
        let requests = self.policy.choose_projects(
            &self.graph,
            &self.state,
            &self.owned_units,
            &self.active_projects,
            self.goal,
        );

        for req in requests {
            let already_active = self
                .active_projects
                .iter()
                .any(|p| p.target_id.eq_ignore_ascii_case(&req.target_id));
            if already_active {
                continue;
            }
            let Some(target) = self.index.find_unit(&req.target_id) else {
                continue;
            };
            if let Some(project) = ActiveProject::new(target, req.requested_bp, req.priority) {
                self.active_projects.push(project);
            }
        }

        if self.active_projects.is_empty() {
            self.current_time += self.dt;
            self.apply_idle_income();
            return;
        }

        // Allocate available BP proportionally to requested BP.
        let total_available = self.available_bp().0;
        let total_requested: f64 = self.active_projects.iter().map(|p| p.requested_bp.0).sum();
        let allocation_factor = if total_requested > 0.0 {
            (total_available / total_requested).min(1.0)
        } else {
            0.0
        };

        for project in &mut self.active_projects {
            let allocated = project.requested_bp.0 * allocation_factor;
            project.project.assigned_build_power = RequestedBuildPower(allocated);
        }

        // Tick projects sequentially. This is a slight approximation.
        for i in 0..self.active_projects.len() {
            self.active_projects[i]
                .project
                .tick(&mut self.state, self.dt);
        }

        self.current_time += self.dt;

        // Complete projects and update state.
        let mut completed = Vec::new();
        for (i, project) in self.active_projects.iter().enumerate() {
            if project.project.is_complete() {
                completed.push(i);
            }
        }

        completed.sort_by(|a, b| b.cmp(a));
        for i in completed {
            let project = self.active_projects.remove(i);
            let Some(target) = self.index.find_unit(&project.target_id) else {
                continue;
            };
            self.owned_units.push(target);
            self.state = derive_economy(&self.owned_units);
            self.events.push(BuildEvent {
                time: self.current_time,
                unit_id: project.target_id.clone(),
                unit_name: target.display_name(),
            });
        }
    }

    /// Collect income for one tick with no projects draining resources.
    fn apply_idle_income(&mut self) {
        self.state.mass_storage = (self.state.mass_storage + self.state.net_mass_income * self.dt)
            .min(self.state.mass_storage_cap)
            .max(0.0);
        self.state.energy_storage = (self.state.energy_storage
            + self.state.net_energy_income * self.dt)
            .min(self.state.energy_storage_cap)
            .max(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_index() -> DataIndex {
        let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
        serde_json::from_str(json).expect("embedded index should parse")
    }

    #[test]
    fn simulator_finishes_monkeylord_with_greedy_policy() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let monkeylord = index.find_unit("URL0402").expect("Monkeylord exists");

        use crate::greedy::StateMachinePolicy;
        let mut sim = HeuristicSimulator::new(
            &index,
            vec![acu],
            monkeylord,
            StateMachinePolicy::default(),
            1.0,
        );
        let goal_event = sim.run().expect("simulation should finish");

        assert_eq!(goal_event.unit_id, "URL0402");
        assert!(goal_event.time > 0.0);
    }
}
