//! Greedy state-machine build policy.
//!
//! This module implements `StateMachinePolicy`, the strategy behind
//! `Strategy::Greedy`. It increases mass income, switches to build power when
//! mass piles up, and builds energy whenever the current build power cannot be
//! sustained.

use faf_units::{BuildTargetStats, Unit};

use crate::economy::{
    compute_drain, total_build_power, EconomyState, RequestedBuildPower, ResourceProducer,
};
use crate::simulator::{ActiveProject, BuildPolicy, ProjectPriority, ProjectRequest};
use crate::tech_graph::TechGraph;

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
        graph: &'a TechGraph<'a>,
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

    /// True if any owned unit can build `target` according to the capability
    /// graph.
    fn can_build_now<'a>(
        &self,
        graph: &'a TechGraph<'a>,
        owned: &[&'a Unit],
        target: &'a Unit,
    ) -> bool {
        let Ok(builders) = graph.builders_for(&target.id) else {
            return false;
        };
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
        graph: &'a TechGraph<'a>,
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

    /// True if `unit` produces energy.
    fn is_energy_producer(&self, unit: &Unit) -> bool {
        unit.has_category("ENERGYPRODUCTION")
    }

    /// Best producer matching `predicate` according to `metric`.
    ///
    /// This treats mass extractors, power generators, and their upgrades as a
    /// single family of resource producers: they all expose build stats,
    /// production, and maintenance, so the planner can pick by efficiency
    /// instead of special-casing each category.
    fn pick_best_producer<'a>(
        &self,
        graph: &'a TechGraph<'a>,
        owned: &[&'a Unit],
        goal: &'a Unit,
        predicate: impl Fn(&Unit) -> bool,
        metric: impl Fn(&ResourceProducer) -> f64,
    ) -> Option<&'a Unit> {
        let goal_faction = goal.faction();
        graph
            .index()
            .units
            .iter()
            .filter(|u| {
                predicate(u)
                    && self.can_build_now(graph, owned, u)
                    && match goal_faction {
                        Some(f) => u
                            .faction()
                            .map_or(true, |uf: &str| uf.eq_ignore_ascii_case(f)),
                        None => true,
                    }
            })
            .filter_map(|u| ResourceProducer::new(u))
            .max_by(|a, b| {
                metric(a)
                    .partial_cmp(&metric(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|p| p.unit())
    }

    /// Most efficient buildable energy producer (energy income per mass cost).
    fn pick_best_energy<'a>(
        &self,
        graph: &'a TechGraph<'a>,
        owned: &[&'a Unit],
        goal: &'a Unit,
    ) -> Option<&'a Unit> {
        self.pick_best_producer(
            graph,
            owned,
            goal,
            |u| self.is_energy_producer(u),
            |p| p.energy_efficiency(),
        )
    }

    /// Most efficient buildable mass extractor (mass income per mass cost).
    fn pick_best_mex<'a>(
        &self,
        graph: &'a TechGraph<'a>,
        owned: &[&'a Unit],
        goal: &'a Unit,
    ) -> Option<&'a Unit> {
        self.pick_best_producer(
            graph,
            owned,
            goal,
            |u| self.is_mex(u),
            |p| p.mass_efficiency(),
        )
    }

    /// Cheapest real builder. Prefer T1 engineers; fall back to factories if
    /// no engineer is currently desirable.
    fn pick_cheapest_builder<'a>(
        &self,
        graph: &'a TechGraph<'a>,
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
        graph: &'a TechGraph<'a>,
        state: &EconomyState,
        owned: &[&'a Unit],
        active: &[ActiveProject],
        goal: &'a Unit,
    ) -> Vec<ProjectRequest> {
        let mut requests = Vec::new();

        // Always keep the goal project active if it is not already. The goal is
        // treated as always buildable so that the simulator can start on it
        // immediately with the starting ACU, matching the current demo behavior.
        // A real planner would instead require the proper tech chain first.
        let goal_active = active.iter().any(|p| p.priority == ProjectPriority::Goal);
        if !goal_active {
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
            ProductionFocus::Energy => self.pick_best_energy(graph, owned, goal),
            ProductionFocus::Mass => self.pick_best_mex(graph, owned, goal),
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

#[cfg(test)]
mod tests {
    use super::*;
    use faf_units::DataIndex;

    fn load_index() -> DataIndex {
        let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
        serde_json::from_str(json).expect("embedded index should parse")
    }

    #[test]
    fn greedy_policy_respects_mex_cap() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let monkeylord = index.find_unit("URL0402").expect("Monkeylord exists");

        let policy = StateMachinePolicy {
            max_mex_count: 2,
            ..Default::default()
        };

        use crate::simulator::HeuristicSimulator;
        let mut sim = HeuristicSimulator::new(&index, vec![acu], monkeylord, policy, 1.0);
        sim.run().expect("simulation should finish");

        let mex_count = sim
            .events
            .iter()
            .filter(|e| {
                sim.index
                    .find_unit(&e.unit_id)
                    .map_or(false, |u| u.has_category("MASSEXTRACTION"))
            })
            .count();
        assert!(mex_count <= 2, "built {} mexes, cap was 2", mex_count);
    }

    #[test]
    fn base_acu_cannot_build_t3_pgen_as_economy() {
        let index = load_index();
        let acu = index.find_unit("URL0001").expect("ACU exists");
        let monkeylord = index.find_unit("URL0402").expect("Monkeylord exists");

        let policy = StateMachinePolicy::default();
        let graph = TechGraph::new(&index);
        let target = policy.pick_best_energy(&graph, &[acu], monkeylord);

        assert_eq!(
            target.map(|u| u.id.as_str()),
            Some("URB1101"),
            "base ACU should only build T1 pgen, not T3"
        );
    }

    #[test]
    fn t3_engineer_prefers_t3_pgen_by_efficiency() {
        let index = load_index();
        let t3_eng = index.find_unit("URL0309").expect("T3 engineer exists");
        let monkeylord = index.find_unit("URL0402").expect("Monkeylord exists");

        let policy = StateMachinePolicy::default();
        let graph = TechGraph::new(&index);
        let target = policy.pick_best_energy(&graph, &[t3_eng], monkeylord);

        assert_eq!(
            target.map(|u| u.id.as_str()),
            Some("URB1301"),
            "T3 engineer should prefer T3 pgen by energy-per-mass efficiency"
        );
    }
}
