//! Shared scheduler output types.
//!
//! These types are used by both the scheduler backend and the web frontend so
//! that the JSON serialization format stays in sync. Keeping them in one place
//! prevents the "missing field" class of frontend errors when the scheduler
//! evolves.

use serde::{Deserialize, Serialize};

use crate::economy_types::EcoSnapshot;
use crate::plan_types::ConstructionPlan;

/// A single step in the planned build order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepResult {
    pub action: Action,
    pub finish_time_seconds: f64,
    /// Number of builder units assigned to this step. For scheduler-generated
    /// steps this is typically one; it is preserved so the timeline can show
    /// actionable descriptions like "4 Engineers build X".
    pub builder_count: usize,
    pub economy: EcoSnapshot,
}

/// A concrete action the scheduler decided to take.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    Build {
        target: faf_blueprints::UnitKind,
        builder: Vec<faf_blueprints::UnitKind>,
    },
    Upgrade {
        from: faf_blueprints::UnitKind,
        to: faf_blueprints::UnitKind,
        assisted_by: Vec<faf_blueprints::UnitKind>,
    },
}

/// The full planned schedule returned by a scheduling run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    pub plan: ConstructionPlan,
    pub total_time_seconds: f64,
    pub final_eco: EcoSnapshot,
    pub steps: Vec<StepResult>,
}

/// A candidate action that was considered for a scheduling step, together with
/// the score it received.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateReasoning {
    pub action: Action,
    pub score: f64,
}

/// Reasoning data for a single committed scheduling step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepReasoning {
    /// Identifier of the committed step (matches the task id assigned during
    /// search).
    pub step_id: u32,
    /// The action that was actually chosen.
    pub chosen: Action,
    /// Highest-scoring candidates considered for this step, sorted from best to
    /// worst.
    pub top_candidates: Vec<CandidateReasoning>,
}

/// A schedule plus the per-step candidate reasoning used to produce it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduleWithReasoning {
    pub schedule: Schedule,
    pub reasoning: Vec<StepReasoning>,
}

/// Errors that can be returned by the scheduler.
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
pub enum ScheduleError {
    #[error("algorithm {0:?} is not implemented yet")]
    AlgorithmNotImplemented(String),
    #[error("no legal builder available for target {target:?}")]
    NoLegalBuilder { target: faf_blueprints::UnitKind },
    #[error("the requested goal is unreachable")]
    GoalUnreachable,
    #[error("the plan stalled during simulation")]
    SimulationStalled,
    #[error("the search timed out")]
    SearchTimeout,
}

#[cfg(test)]
mod tests {
    use super::*;
    use faf_blueprints::{TechLevel, UnitKind};
    use faf_quantities::{Energy, EnergyRate, Mass, MassRate, Time};

    #[test]
    fn action_upgrade_serializes_without_builder_field() {
        let action = Action::Upgrade {
            from: UnitKind::Mex(TechLevel::T1),
            to: UnitKind::Mex(TechLevel::T2),
            assisted_by: vec![],
        };
        let value = serde_json::to_value(&action).unwrap();
        let variant = value
            .as_object()
            .unwrap()
            .get("Upgrade")
            .unwrap()
            .as_object()
            .unwrap();
        assert!(
            variant.get("builder").is_none(),
            "Action::Upgrade must not carry a `builder` field after the upgrade refactor"
        );
        assert!(variant.contains_key("from"));
        assert!(variant.contains_key("to"));
        assert!(variant.contains_key("assisted_by"));
    }

    #[test]
    fn action_roundtrips_through_json() {
        let actions = vec![
            Action::Build {
                target: UnitKind::Engineer(TechLevel::T1),
                builder: vec![UnitKind::Factory(TechLevel::T1)],
            },
            Action::Upgrade {
                from: UnitKind::Mex(TechLevel::T1),
                to: UnitKind::Mex(TechLevel::T2),
                assisted_by: vec![UnitKind::Engineer(TechLevel::T1)],
            },
        ];
        for action in actions {
            let json = serde_json::to_string(&action).unwrap();
            let decoded: Action = serde_json::from_str(&json).unwrap();
            assert_eq!(action, decoded);
        }
    }

    #[test]
    fn schedule_roundtrips_through_json() {
        let snapshot = EcoSnapshot {
            time: Time::from_raw(0.0),
            production_per_second_mass: MassRate::from_raw(1.0),
            production_per_second_energy: EnergyRate::from_raw(20.0),
            maintenance_consumption_per_second_energy: EnergyRate::from_raw(0.0),
            mass_drain: MassRate::from_raw(0.0),
            energy_drain: EnergyRate::from_raw(0.0),
            total_mass_spent: Mass::from_raw(0.0),
            total_energy_spent: Energy::from_raw(0.0),
            mass_storage: Mass::from_raw(650.0),
            mass_storage_cap: Mass::from_raw(650.0),
            energy_storage: Energy::from_raw(4000.0),
            energy_storage_cap: Energy::from_raw(4000.0),
        };
        let schedule = Schedule {
            plan: ConstructionPlan::default(),
            total_time_seconds: 42.0,
            final_eco: snapshot,
            steps: vec![StepResult {
                action: Action::Upgrade {
                    from: UnitKind::Mex(TechLevel::T1),
                    to: UnitKind::Mex(TechLevel::T2),
                    assisted_by: vec![],
                },
                finish_time_seconds: 12.0,
                builder_count: 1,
                economy: snapshot,
            }],
        };
        let json = serde_json::to_string(&schedule).unwrap();
        let decoded: Schedule = serde_json::from_str(&json).unwrap();
        assert_eq!(schedule, decoded);
    }
}
