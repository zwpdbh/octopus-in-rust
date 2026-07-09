//! Rush planner: assess whether a goal can be finished within a time window.
//!
//! The rush planner does not produce a build order.  Given a [`SimulationState`],
//! a [`Goal`], and a `time_window` in seconds, it answers two questions:
//!
//! 1. Can the goal be completed within `time_window` if we start it now?
//! 2. If not, what mass-income level do we need to reach first?
//!
//! The first question is answered by a short simulator rollout that assigns idle
//! T3 engineers to the goal.  The second is answered by binary-searching mass
//! income with the economy's continuous-time estimator.

use crate::engine::{NodeId, SimulationState};
use crate::planner::core::{Goal, PlannerConfig};
use crate::units::{TechLevel, UnitKind, Units};
use faf_units::BuildTargetStats;

/// Result of a rush-feasibility assessment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RushAssessment {
    /// True if the rollout completed the goal within the requested window.
    pub can_finish: bool,
    /// Estimated time to finish at the *current* economy, or `None` if the
    /// rollout could not determine it.
    pub expected_finish_time: Option<f64>,
    /// Mass income (mass/second) that must be reached before attempting the
    /// goal if `can_finish` is false.  `f64::INFINITY` means mass income alone
    /// cannot meet the deadline (energy or build power is the bottleneck).
    pub required_mass_income: f64,
}

impl RushAssessment {
    /// Human-readable summary used by CLI and tests.
    pub fn summary(&self) -> String {
        if self.can_finish {
            if let Some(t) = self.expected_finish_time {
                format!("rush feasible, estimated finish in {:.1}s", t)
            } else {
                "rush feasible".to_string()
            }
        } else if self.required_mass_income.is_finite() {
            format!(
                "rush infeasible; need {:.1} mass/s first",
                self.required_mass_income
            )
        } else {
            "rush infeasible; mass income alone is not the bottleneck".to_string()
        }
    }
}

/// Planner that assesses whether a goal can be rushed within a time window.
#[derive(Debug, Clone)]
pub struct RushPlanner {
    config: PlannerConfig,
}

impl RushPlanner {
    /// Create a rush planner with the given simulation configuration.
    pub fn new(config: PlannerConfig) -> Self {
        Self { config }
    }

    /// Shared planner configuration.
    pub fn config(&self) -> &PlannerConfig {
        &self.config
    }

    /// Assess whether `goal` can be finished from `state` within `time_window`
    /// seconds.
    ///
    /// The method clones `state`, starts a goal project with all idle T3
    /// engineers, and runs a simulator rollout up to `time_window`.  If the
    /// goal completes during the rollout, [`RushAssessment::can_finish`] is
    /// true and `expected_finish_time` is the simulated completion time.
    ///
    /// If the rollout does not complete the goal, `expected_finish_time` is
    /// filled by the economy's continuous-time estimator at the current state,
    /// and `required_mass_income` is the mass-income level that (holding energy
    /// income and build power constant) would bring that estimate down to
    /// `time_window`.
    pub fn assess(
        &self,
        state: &SimulationState,
        units: &Units,
        goal: &Goal,
        time_window: f64,
    ) -> RushAssessment {
        if time_window <= 0.0 || state.goal_reached(goal) {
            return RushAssessment {
                can_finish: state.goal_reached(goal),
                expected_finish_time: Some(0.0),
                required_mass_income: state.economy.net_mass_income.value(),
            };
        }

        let mut rollout_state = state.clone();
        let builders = select_goal_builders(&rollout_state, units);

        if builders.is_empty() {
            // No T3 engineers available to start the goal.
            return RushAssessment {
                can_finish: false,
                expected_finish_time: None,
                required_mass_income: f64::INFINITY,
            };
        }

        if rollout_state
            .start_goal_project(*goal, &builders, units)
            .is_err()
        {
            return RushAssessment {
                can_finish: false,
                expected_finish_time: None,
                required_mass_income: f64::INFINITY,
            };
        }

        let rollout_result =
            run_rush_rollout(&mut rollout_state, units, time_window, self.config.dt);

        if rollout_result.completed {
            return RushAssessment {
                can_finish: true,
                expected_finish_time: Some(rollout_result.completion_time_secs),
                required_mass_income: state.economy.net_mass_income.value(),
            };
        }

        // Rollout failed: estimate required mass income.
        let build_power = state.total_active_build_power(units);
        let cost = goal.cost().to_target_stats();
        let current_estimate = state.economy.estimate_remaining_time(cost, build_power);

        let required_mass_income = if current_estimate <= time_window {
            // Static estimator thinks it fits even though the rollout did not;
            // report the current income as sufficient.
            state.economy.net_mass_income.value()
        } else {
            required_mass_income_for_deadline(&state.economy, cost, build_power, time_window)
        };

        RushAssessment {
            can_finish: false,
            expected_finish_time: Some(current_estimate),
            required_mass_income,
        }
    }
}

impl Default for RushPlanner {
    fn default() -> Self {
        Self::new(PlannerConfig::default())
    }
}

/// Select all idle T3 engineers, highest build-rate first.
fn select_goal_builders(state: &SimulationState, units: &Units) -> Vec<NodeId> {
    let mut candidates: Vec<NodeId> = state
        .idle_builders(units)
        .into_iter()
        .filter(|&id| matches!(state.graph[id].unit_id, UnitKind::Engineer(TechLevel::T3)))
        .collect();

    candidates.sort_by(|&a, &b| {
        let rate_a = units
            .def(&state.graph[a].unit_id)
            .map(|d| d.build_rate())
            .unwrap_or(0.0);
        let rate_b = units
            .def(&state.graph[b].unit_id)
            .map(|d| d.build_rate())
            .unwrap_or(0.0);
        rate_b.total_cmp(&rate_a)
    });

    candidates
}

/// Outcome of the internal rush rollout.
struct RolloutOutcome {
    completed: bool,
    completion_time_secs: f64,
}

/// Run `state` forward for up to `time_window` seconds, stopping when the goal
/// project completes.
fn run_rush_rollout(
    state: &mut SimulationState,
    units: &Units,
    time_window: f64,
    dt: f64,
) -> RolloutOutcome {
    let start_time = state.time;
    let steps = ((time_window / dt).ceil() as usize).max(1);

    for _ in 0..steps {
        if goal_project_completed(state) {
            return RolloutOutcome {
                completed: true,
                completion_time_secs: state.time - start_time,
            };
        }
        state.tick(units, dt);
    }

    RolloutOutcome {
        completed: goal_project_completed(state),
        completion_time_secs: state.time - start_time,
    }
}

/// True if the active goal project is marked completed.
fn goal_project_completed(state: &SimulationState) -> bool {
    state.goal_project.as_ref().is_some_and(|p| p.completed)
}

/// Binary-search the mass income that brings `estimate_remaining_time` down to
/// `deadline` seconds.
///
/// Energy income, storage, and build power are held constant.  If no finite
/// mass income can meet the deadline, returns `f64::INFINITY`.
fn required_mass_income_for_deadline(
    economy: &crate::economy::EconomyState,
    cost: BuildTargetStats,
    build_power: f64,
    deadline: f64,
) -> f64 {
    if build_power <= 0.0 {
        return f64::INFINITY;
    }

    let mut lower = economy.net_mass_income.value().max(0.0);
    let mut upper = (cost.build_cost_mass / deadline.max(1e-6)).max(lower * 2.0 + 1.0);
    const MAX_INCOME: f64 = 1_000_000.0;

    // Expand upper bound until the deadline is reachable or we hit the ceiling.
    loop {
        let trial = hypothetical_economy(economy, upper);
        let t = trial.estimate_remaining_time(cost, build_power);
        if t <= deadline {
            break;
        }
        if upper >= MAX_INCOME {
            return f64::INFINITY;
        }
        lower = upper;
        upper *= 2.0;
    }

    // Binary search for the minimum required mass income.
    for _ in 0..50 {
        let mid = (lower + upper) / 2.0;
        let trial = hypothetical_economy(economy, mid);
        let t = trial.estimate_remaining_time(cost, build_power);
        if t <= deadline {
            upper = mid;
        } else {
            lower = mid;
        }
    }

    upper
}

/// Build an economy identical to `base` except for mass income.
fn hypothetical_economy(
    base: &crate::economy::EconomyState,
    mass_income: f64,
) -> crate::economy::EconomyState {
    crate::economy::EconomyState {
        net_mass_income: crate::quantities::MassRate::from_raw(mass_income),
        ..*base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{TechLevel, UnitKind, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    fn t4_goal() -> Goal {
        Goal {
            tech_level: TechLevel::T4,
            mass_cost: 28_000.0,
            energy_cost: 340_000.0,
            build_time: 46_250.0,
        }
    }

    #[test]
    fn rush_infeasible_from_acu_alone() {
        let units = load_units();
        let state = SimulationState::new(&units, &[UnitKind::Commander]);

        let planner = RushPlanner::default();
        let assessment = planner.assess(&state, &units, &t4_goal(), 300.0);

        assert!(!assessment.can_finish);
        // Without a T3 engineer the goal cannot even be started, so the planner
        // reports that mass income alone is not the bottleneck.
        assert!(
            !assessment.required_mass_income.is_finite(),
            "ACU alone cannot start the goal; required income should be infinity"
        );
    }

    #[test]
    fn rush_feasible_with_many_t3_engineers() {
        let units = load_units();
        let mut state = SimulationState::new(
            &units,
            &[
                UnitKind::Commander,
                UnitKind::Engineer(TechLevel::T3),
                UnitKind::Engineer(TechLevel::T3),
                UnitKind::Engineer(TechLevel::T3),
                UnitKind::Engineer(TechLevel::T3),
                UnitKind::Engineer(TechLevel::T3),
                UnitKind::Engineer(TechLevel::T3),
                UnitKind::Engineer(TechLevel::T3),
                UnitKind::Engineer(TechLevel::T3),
                UnitKind::Engineer(TechLevel::T3),
                UnitKind::Engineer(TechLevel::T3),
            ],
        );
        // Give the economy enough resources to support the build.
        state.economy.mass_storage = crate::quantities::Mass::from_raw(50_000.0);
        state.economy.mass_storage_cap = crate::quantities::Mass::from_raw(100_000.0);
        state.economy.energy_storage = crate::quantities::Energy::from_raw(400_000.0);
        state.economy.energy_storage_cap = crate::quantities::Energy::from_raw(500_000.0);
        state.economy.net_mass_income = crate::quantities::MassRate::from_raw(100.0);
        state.economy.net_energy_income = crate::quantities::EnergyRate::from_raw(5_000.0);

        let planner = RushPlanner::default();
        let assessment = planner.assess(&state, &units, &t4_goal(), 600.0);

        assert!(
            assessment.can_finish,
            "many T3 engineers with resources should finish the goal, got {:?}",
            assessment
        );
        assert!(assessment.expected_finish_time.is_some());
    }
}
