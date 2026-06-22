//! Heuristic simulator with a state-machine build policy.
//!
//! The simulator runs multiple projects at once (e.g., building engineers while
//! they assist the main target). The default policy is a small state machine
//! that decides whether to build energy, mass, or build power next.

use faf_units::{BuildTargetStats, DataIndex, Unit};

use crate::build_graph::BuildGraph;
use crate::economy::{
    compute_drain, total_build_power, BuildProject, EconomyState, RequestedBuildPower,
};
use crate::sim::{derive_economy, BuildEvent};

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

/// A project currently being built by the heuristic simulator.
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
    fn new(
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
        graph: &'a BuildGraph<'a>,
        state: &EconomyState,
        owned: &[&'a Unit],
        active: &[ActiveProject],
        goal: &'a Unit,
    ) -> Vec<ProjectRequest>;
}

/// What the economy needs most right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductionFocus {
    /// Build power generators until energy can sustain the current build power.
    Energy,
    /// Build mass extractors while we have the energy to support them.
    Mass,
    /// Build engineers/factories because mass is piling up faster than we can
    /// spend it.
    BuildPower,
}

/// State-machine policy: increase mass income as much as possible, but switch
/// to build power when mass is piling up, and build energy whenever the current
/// build power cannot be sustained.
#[derive(Debug, Clone, Copy)]
pub struct StateMachinePolicy {
    /// Maximum total number of mass extractors (owned + under construction).
    pub max_mex_count: usize,
    /// Net energy income must cover at least this fraction of the energy drain
    /// if all available build power is applied to the goal.
    pub energy_safety_margin: f64,
    /// Mass income is considered excessive when it exceeds this multiple of
    /// the mass we could consume by applying all BP to the goal.
    pub mass_income_headroom: f64,
    /// Mass storage fraction above which we switch to build power.
    pub mass_storage_high: f64,
    /// Build power assigned to each secondary (builder/economy) project.
    pub secondary_bp: RequestedBuildPower,
    /// Build power assigned to the main goal project.
    pub goal_bp: RequestedBuildPower,
}

impl Default for StateMachinePolicy {
    fn default() -> Self {
        Self {
            max_mex_count: 8,
            energy_safety_margin: 1.1,
            mass_income_headroom: 1.0,
            mass_storage_high: 0.8,
            // One T1 engineer's worth of BP for each secondary project.
            secondary_bp: RequestedBuildPower(5.0),
            // Ask for a lot; proportional allocation will give it the leftovers.
            goal_bp: RequestedBuildPower(1_000.0),
        }
    }
}

impl StateMachinePolicy {
    /// True if `unit` is a mass extractor of any tech level.
    fn is_mex(&self, unit: &Unit) -> bool {
        unit.has_category("MASSEXTRACTION")
    }

    /// Current number of mass extractors, counting owned units and active
    /// projects.
    fn current_mex_count<'a>(
        &self,
        graph: &'a BuildGraph<'a>,
        owned: &[&'a Unit],
        active: &[ActiveProject],
    ) -> usize {
        let owned_mex = owned.iter().filter(|u| self.is_mex(u)).count();
        let active_mex = active
            .iter()
            .filter(|p| {
                graph
                    .index()
                    .find_unit(&p.target_id)
                    .map_or(false, |u| self.is_mex(u))
            })
            .count();
        owned_mex + active_mex
    }

    /// True if any owned unit can build `target` according to the graph.
    fn can_build_now<'a>(
        &self,
        graph: &'a BuildGraph<'a>,
        owned: &[&'a Unit],
        target: &'a Unit,
    ) -> bool {
        let builders = graph.builders_for(&target.id);
        owned.iter().any(|owned| {
            builders
                .iter()
                .any(|b| b.id.eq_ignore_ascii_case(&owned.id))
        })
    }

    /// Drain per build power for the goal unit.
    fn goal_drain_per_bp(&self, goal: &Unit) -> Option<BuildTargetStats> {
        goal.build_target_stats()
    }

    /// Determine the current economic focus.
    fn focus<'a>(
        &self,
        graph: &'a BuildGraph<'a>,
        state: &EconomyState,
        owned: &[&'a Unit],
        active: &[ActiveProject],
        goal: &'a Unit,
    ) -> ProductionFocus {
        let bp = total_build_power(owned).0;
        let Some(stats) = self.goal_drain_per_bp(goal) else {
            return ProductionFocus::BuildPower;
        };
        let Some(drain) = compute_drain(&stats, RequestedBuildPower(1.0)) else {
            return ProductionFocus::BuildPower;
        };

        // 1. Energy sustainability check.
        let energy_drain_at_full_bp = bp * drain.energy_per_second;
        if state.net_energy_income < energy_drain_at_full_bp * self.energy_safety_margin {
            return ProductionFocus::Energy;
        }

        // 2. Mass income check: are we producing more mass than we can spend?
        let mass_drain_at_full_bp = bp * drain.mass_per_second;
        let mass_income_high =
            state.net_mass_income > mass_drain_at_full_bp * self.mass_income_headroom;
        let mass_storage_high = state.mass_storage_cap > 0.0
            && state.mass_storage > state.mass_storage_cap * self.mass_storage_high;

        if mass_income_high || mass_storage_high {
            return ProductionFocus::BuildPower;
        }

        // 3. Default: expand mass income, unless we are already at the mex cap.
        if self.current_mex_count(graph, owned, active) < self.max_mex_count {
            return ProductionFocus::Mass;
        }

        // At mex cap with enough energy: build power to spend the mass.
        ProductionFocus::BuildPower
    }

    /// Cheapest unit that produces energy.
    fn pick_cheapest_energy<'a>(
        &self,
        graph: &'a BuildGraph<'a>,
        owned: &[&'a Unit],
        goal: &'a Unit,
    ) -> Option<&'a Unit> {
        let goal_faction = goal.faction();
        graph
            .index()
            .units
            .iter()
            .filter(|u| {
                u.economy.as_ref().map_or(false, |e| {
                    e.production_per_second_energy.unwrap_or(0.0) > 0.0
                }) && self.can_build_now(graph, owned, u)
                    && match goal_faction {
                        Some(f) => u
                            .faction()
                            .map_or(true, |uf: &str| uf.eq_ignore_ascii_case(f)),
                        None => true,
                    }
            })
            .min_by(|a, b| {
                let a_cost = a
                    .build_target_stats()
                    .map_or(f64::INFINITY, |s| s.build_cost_mass);
                let b_cost = b
                    .build_target_stats()
                    .map_or(f64::INFINITY, |s| s.build_cost_mass);
                a_cost
                    .partial_cmp(&b_cost)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// Cheapest mass extractor.
    fn pick_cheapest_mex<'a>(
        &self,
        graph: &'a BuildGraph<'a>,
        owned: &[&'a Unit],
        goal: &'a Unit,
    ) -> Option<&'a Unit> {
        let goal_faction = goal.faction();
        graph
            .index()
            .units
            .iter()
            .filter(|u| {
                self.is_mex(u)
                    && self.can_build_now(graph, owned, u)
                    && match goal_faction {
                        Some(f) => u
                            .faction()
                            .map_or(true, |uf: &str| uf.eq_ignore_ascii_case(f)),
                        None => true,
                    }
            })
            .min_by(|a, b| {
                let a_cost = a
                    .build_target_stats()
                    .map_or(f64::INFINITY, |s| s.build_cost_mass);
                let b_cost = b
                    .build_target_stats()
                    .map_or(f64::INFINITY, |s| s.build_cost_mass);
                a_cost
                    .partial_cmp(&b_cost)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// Cheapest real builder. Prefer T1 engineers; fall back to factories if
    /// no engineer is currently desirable.
    fn pick_cheapest_builder<'a>(
        &self,
        graph: &'a BuildGraph<'a>,
        owned: &[&'a Unit],
        goal: &'a Unit,
    ) -> Option<&'a Unit> {
        let goal_faction = goal.faction();
        let mut candidates: Vec<&Unit> = graph
            .index()
            .units
            .iter()
            .filter(|u| {
                u.builder_capability()
                    .map_or(false, |cap| cap.build_rate > 0.0)
                    && (u.has_category("ENGINEER") || u.has_category("FACTORY"))
                    && self.can_build_now(graph, owned, u)
                    && match goal_faction {
                        Some(f) => u
                            .faction()
                            .map_or(true, |uf: &str| uf.eq_ignore_ascii_case(f)),
                        None => true,
                    }
            })
            .collect();

        // Prefer engineers over factories.
        candidates.sort_by(|a, b| {
            let a_is_eng = a.has_category("ENGINEER");
            let b_is_eng = b.has_category("ENGINEER");
            match (a_is_eng, b_is_eng) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => {
                    let a_stats = a.build_target_stats().unwrap();
                    let a_cap = a.builder_capability().unwrap();
                    let b_stats = b.build_target_stats().unwrap();
                    let b_cap = b.builder_capability().unwrap();
                    let a_time_per_bp = a_stats.build_time / a_cap.build_rate;
                    let b_time_per_bp = b_stats.build_time / b_cap.build_rate;
                    a_time_per_bp
                        .partial_cmp(&b_time_per_bp)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }
            }
        });

        candidates.into_iter().next()
    }
}

impl BuildPolicy for StateMachinePolicy {
    fn choose_projects<'a>(
        &self,
        graph: &'a BuildGraph<'a>,
        state: &EconomyState,
        owned: &[&'a Unit],
        active: &[ActiveProject],
        goal: &'a Unit,
    ) -> Vec<ProjectRequest> {
        let mut requests = Vec::new();

        // Always keep the goal project active if it is not already.
        let goal_active = active.iter().any(|p| p.priority == ProjectPriority::Goal);
        if !goal_active && self.can_build_now(graph, owned, goal) {
            requests.push(ProjectRequest {
                target_id: goal.id.clone(),
                requested_bp: self.goal_bp,
                priority: ProjectPriority::Goal,
            });
        }

        // Only one secondary project per tick to keep the state machine simple.
        let has_secondary = active.iter().any(|p| {
            p.priority == ProjectPriority::Builder || p.priority == ProjectPriority::Economy
        });
        if has_secondary {
            return requests;
        }

        let focus = self.focus(graph, state, owned, active, goal);
        let target = match focus {
            ProductionFocus::Energy => self.pick_cheapest_energy(graph, owned, goal),
            ProductionFocus::Mass => self.pick_cheapest_mex(graph, owned, goal),
            ProductionFocus::BuildPower => self.pick_cheapest_builder(graph, owned, goal),
        };

        if let Some(target) = target {
            let priority = match focus {
                ProductionFocus::Energy | ProductionFocus::Mass => ProjectPriority::Economy,
                ProductionFocus::BuildPower => ProjectPriority::Builder,
            };
            requests.push(ProjectRequest {
                target_id: target.id.clone(),
                requested_bp: self.secondary_bp,
                priority,
            });
        }

        requests
    }
}

/// Heuristic simulator that runs concurrent projects under a policy.
pub struct HeuristicSimulator<'a, P: BuildPolicy> {
    /// Unit database.
    pub index: &'a DataIndex,
    /// Dependency graph.
    pub graph: BuildGraph<'a>,
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
    /// Create a new heuristic simulator.
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
            graph: BuildGraph::new(index),
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
                unit_name: target.name().map(|s: &str| s.to_string()),
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
    fn heuristic_finishes_monkeylord() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let monkeylord = index.find_unit("URL0402").expect("Monkeylord exists");

        let mut heuristic = HeuristicSimulator::new(
            &index,
            vec![acu],
            monkeylord,
            StateMachinePolicy::default(),
            1.0,
        );
        let goal_event = heuristic.run().expect("heuristic should finish");

        assert_eq!(goal_event.unit_id, "URL0402");
        assert!(goal_event.time > 0.0);
    }

    #[test]
    fn heuristic_respects_mex_cap() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let monkeylord = index.find_unit("URL0402").expect("Monkeylord exists");

        let policy = StateMachinePolicy {
            max_mex_count: 2,
            ..Default::default()
        };

        let mut heuristic = HeuristicSimulator::new(&index, vec![acu], monkeylord, policy, 1.0);
        heuristic.run().expect("heuristic should finish");

        let mex_count = heuristic
            .events
            .iter()
            .filter(|e| {
                heuristic
                    .index
                    .find_unit(&e.unit_id)
                    .map_or(false, |u| u.has_category("MASSEXTRACTION"))
            })
            .count();
        assert!(mex_count <= 2, "built {} mexes, cap was 2", mex_count);
    }
}
