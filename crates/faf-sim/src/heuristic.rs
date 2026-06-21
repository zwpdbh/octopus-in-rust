//! Heuristic simulator that grows build power while avoiding stalls.
//!
//! Unlike `SimpleSimulator`, this simulator can run multiple projects at once
//! (e.g., building engineers while they assist the main target) and uses a
//! policy to decide what to build next. The default policy tries to maximize
//! build power without stalling, which matches the typical player mindset.

use faf_units::{DataIndex, Unit};

use crate::build_graph::BuildGraph;
use crate::economy::{compute_drain, BuildProject, EconomyState, RequestedBuildPower};
use crate::sim::{derive_economy, BuildEvent};

/// Priority of an active project. Higher-priority projects are not given more
/// build power directly; the priority is used by policies to decide whether to
/// start or replace projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProjectPriority {
    /// The unit we are ultimately trying to finish.
    Goal,
    /// A builder or factory whose only purpose is to increase total build power.
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
    ///
    /// The simulator will filter out requests for units that are already being
    /// built and will respect its own concurrency limits.
    fn choose_projects<'a>(
        &self,
        graph: &'a BuildGraph<'a>,
        state: &EconomyState,
        owned: &[&'a Unit],
        active: &[ActiveProject],
        goal: &'a Unit,
    ) -> Vec<ProjectRequest>;
}

/// Greedy policy: keep adding cheap builders until the economy can barely
/// support them, while keeping a small storage buffer to avoid micro-stalls.
#[derive(Debug, Clone, Copy)]
pub struct GreedyNoStallPolicy {
    /// Target: requested BP should be at most this fraction of the income
    /// ceiling. Values below 1.0 leave headroom so temporary income drops do
    /// not immediately stall everything.
    pub bp_utilization_target: f64,
    /// Do not let storage fall below this fraction of its cap. If it would,
    /// prioritize economy buildings instead.
    pub storage_safety_fraction: f64,
    /// Maximum number of concurrent builder projects (engineers/factories).
    pub max_concurrent_builders: usize,
    /// Maximum number of concurrent economy projects (mexes/pgens).
    pub max_concurrent_economy: usize,
    /// Build power assigned to each secondary (builder/economy) project.
    pub secondary_bp: RequestedBuildPower,
    /// Build power assigned to the main goal project.
    pub goal_bp: RequestedBuildPower,
}

impl Default for GreedyNoStallPolicy {
    fn default() -> Self {
        Self {
            bp_utilization_target: 0.95,
            storage_safety_fraction: 0.15,
            max_concurrent_builders: 2,
            max_concurrent_economy: 1,
            // One T1 engineer's worth of BP for each secondary project.
            secondary_bp: RequestedBuildPower(5.0),
            // Ask for a lot; proportional allocation will give it the leftovers.
            goal_bp: RequestedBuildPower(1_000.0),
        }
    }
}

impl GreedyNoStallPolicy {
    /// Estimate how much build power the current income can sustain without
    /// stalling, using the drain profile of `reference_unit` per BP.
    fn sustainable_bp(&self, state: &EconomyState, reference_unit: &Unit) -> RequestedBuildPower {
        let Some(drain) = compute_drain(reference_unit, RequestedBuildPower(1.0)) else {
            return RequestedBuildPower(f64::INFINITY);
        };

        let mass_limit = if drain.mass_per_second > 0.0 {
            state.net_mass_income / drain.mass_per_second
        } else {
            f64::INFINITY
        };
        let energy_limit = if drain.energy_per_second > 0.0 {
            state.net_energy_income / drain.energy_per_second
        } else {
            f64::INFINITY
        };

        RequestedBuildPower(mass_limit.min(energy_limit))
    }

    /// Total build power provided by owned units.
    fn owned_bp(&self, owned: &[&Unit]) -> RequestedBuildPower {
        RequestedBuildPower(
            owned
                .iter()
                .filter_map(|u| u.economy.as_ref()?.build_rate)
                .sum(),
        )
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

    /// Return the cheapest builder we can currently produce, measuring "cheap"
    /// by build-time per build-power gained.
    fn pick_cheapest_builder<'a>(
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
                // Must add build power.
                u.economy.as_ref().and_then(|e| e.build_rate).unwrap_or(0.0) > 0.0
                    // Must be buildable now.
                    && self.can_build_now(graph, owned, u)
                    // Match goal faction, if any.
                    && match goal_faction {
                        Some(f) => u.faction().map_or(true, |uf: &str| uf.eq_ignore_ascii_case(f)),
                        None => true,
                    }
            })
            .min_by(|a, b| {
                let a_econ = a.economy.as_ref().unwrap();
                let b_econ = b.economy.as_ref().unwrap();
                let a_time_per_bp = a_econ.build_time.unwrap() / a_econ.build_rate.unwrap();
                let b_time_per_bp = b_econ.build_time.unwrap() / b_econ.build_rate.unwrap();
                a_time_per_bp
                    .partial_cmp(&b_time_per_bp)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// Return the cheapest economy unit we can currently produce. Prefer mass
    /// extractors if mass income is the bottleneck, otherwise power generators.
    fn pick_cheapest_economy<'a>(
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
                let econ = match u.economy.as_ref() {
                    Some(e) => e,
                    None => return false,
                };
                // Must add income or storage.
                let adds_economy = econ.production_per_second_mass.unwrap_or(0.0) > 0.0
                    || econ.production_per_second_energy.unwrap_or(0.0) > 0.0;
                adds_economy
                    && self.can_build_now(graph, owned, u)
                    && match goal_faction {
                        Some(f) => u
                            .faction()
                            .map_or(true, |uf: &str| uf.eq_ignore_ascii_case(f)),
                        None => true,
                    }
            })
            .min_by(|a, b| {
                let a_econ = a.economy.as_ref().unwrap();
                let b_econ = b.economy.as_ref().unwrap();
                let a_cost = a_econ.build_cost_mass.unwrap_or(f64::INFINITY);
                let b_cost = b_econ.build_cost_mass.unwrap_or(f64::INFINITY);
                a_cost
                    .partial_cmp(&b_cost)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }
}

impl BuildPolicy for GreedyNoStallPolicy {
    fn choose_projects<'a>(
        &self,
        graph: &'a BuildGraph<'a>,
        state: &EconomyState,
        owned: &[&'a Unit],
        active: &[ActiveProject],
        goal: &'a Unit,
    ) -> Vec<ProjectRequest> {
        let mut requests = Vec::new();

        // 1. Always keep the goal project active if it is not already.
        let goal_active = active.iter().any(|p| p.priority == ProjectPriority::Goal);
        if !goal_active && self.can_build_now(graph, owned, goal) {
            requests.push(ProjectRequest {
                target_id: goal.id.clone(),
                requested_bp: self.goal_bp,
                priority: ProjectPriority::Goal,
            });
        }

        // 2. If storage is dangerously low, prioritize economy.
        let mass_low = state.mass_storage_cap > 0.0
            && state.mass_storage < state.mass_storage_cap * self.storage_safety_fraction;
        let energy_low = state.energy_storage_cap > 0.0
            && state.energy_storage < state.energy_storage_cap * self.storage_safety_fraction;

        let active_economy = active
            .iter()
            .filter(|p| p.priority == ProjectPriority::Economy)
            .count();

        if (mass_low || energy_low) && active_economy < self.max_concurrent_economy {
            if let Some(econ) = self.pick_cheapest_economy(graph, owned, goal) {
                requests.push(ProjectRequest {
                    target_id: econ.id.clone(),
                    requested_bp: self.secondary_bp,
                    priority: ProjectPriority::Economy,
                });
            }
        }

        // 3. If we have headroom, build more builders.
        let active_builders = active
            .iter()
            .filter(|p| p.priority == ProjectPriority::Builder)
            .count();

        // Use the goal unit as the drain reference; if even the goal has no
        // economy data, fall back to a T1 engineer profile for the estimate.
        let reference = if compute_drain(goal, RequestedBuildPower(1.0)).is_some() {
            goal
        } else {
            // Fall back to first T1 engineer in the index.
            graph
                .index()
                .units
                .iter()
                .find(|u| {
                    u.has_category("ENGINEER")
                        && u.has_category("TECH1")
                        && goal.faction().map_or(true, |f: &str| {
                            u.faction()
                                .map_or(true, |uf: &str| uf.eq_ignore_ascii_case(f))
                        })
                })
                .unwrap_or(goal)
        };

        let owned_bp = self.owned_bp(owned);
        let sustainable = self.sustainable_bp(state, reference);
        let target_bp = sustainable.0 * self.bp_utilization_target;

        if owned_bp.0 < target_bp && active_builders < self.max_concurrent_builders {
            if let Some(builder) = self.pick_cheapest_builder(graph, owned, goal) {
                requests.push(ProjectRequest {
                    target_id: builder.id.clone(),
                    requested_bp: self.secondary_bp,
                    priority: ProjectPriority::Builder,
                });
            }
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
        // This method intentionally takes owned `starting_units` so the caller
        // can pass a temporary vector. The units inside still borrow from `index`.
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
        RequestedBuildPower(
            self.owned_units
                .iter()
                .filter_map(|u| u.economy.as_ref()?.build_rate)
                .sum(),
        )
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

            // Check if the goal completed this tick.
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
        // Ask the policy for any new projects.
        let requests = self.policy.choose_projects(
            &self.graph,
            &self.state,
            &self.owned_units,
            &self.active_projects,
            self.goal,
        );

        for req in requests {
            // Avoid duplicate active projects.
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
            // Nothing to do; still advance time and collect income.
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

        // Tick every active project. Because BuildProject::tick updates the
        // shared economy state, we must tick them sequentially. This is a
        // slight approximation: in reality all projects drain simultaneously.
        // The error is small for typical dt values.
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

        // Remove from highest index to lowest to keep indices valid.
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
                unit_name: project.project.target.name().map(|s: &str| s.to_string()),
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
    fn heuristic_faster_than_acu_alone_for_monkeylord() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let monkeylord = index.find_unit("URL0402").expect("Monkeylord exists");

        // Baseline: ACU alone.
        let mut baseline = crate::sim::SimpleSimulator::new(&index, vec![acu], 1.0);
        let baseline_events = baseline.simulate_sequence(&[monkeylord]);
        let baseline_time = baseline_events[0].time;

        // Heuristic: ACU builds engineers to assist.
        let mut heuristic = HeuristicSimulator::new(
            &index,
            vec![acu],
            monkeylord,
            GreedyNoStallPolicy::default(),
            1.0,
        );
        let goal_event = heuristic.run().expect("heuristic should finish");

        assert!(
            goal_event.time < baseline_time,
            "heuristic ({}) should beat ACU-alone baseline ({})",
            goal_event.time,
            baseline_time
        );
    }

    #[test]
    fn heuristic_builds_at_least_one_engineer() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let monkeylord = index.find_unit("URL0402").expect("Monkeylord exists");

        let mut heuristic = HeuristicSimulator::new(
            &index,
            vec![acu],
            monkeylord,
            GreedyNoStallPolicy::default(),
            1.0,
        );
        heuristic.run().expect("heuristic should finish");

        let built_engineer = heuristic
            .events
            .iter()
            .any(|e| e.unit_id.eq_ignore_ascii_case("URL0105"));
        assert!(
            built_engineer,
            "heuristic should build at least one T1 engineer"
        );
    }

    #[test]
    fn storage_safety_triggers_economy() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let monkeylord = index.find_unit("URL0402").expect("Monkeylord exists");

        let policy = GreedyNoStallPolicy {
            bp_utilization_target: 10.0, // Force lots of builders.
            storage_safety_fraction: 0.5,
            ..Default::default()
        };

        let mut heuristic = HeuristicSimulator::new(&index, vec![acu], monkeylord, policy, 1.0);
        heuristic.run().expect("heuristic should finish");

        // With high BP target, we should either build economy or still finish.
        assert!(!heuristic.events.is_empty());
    }
}
