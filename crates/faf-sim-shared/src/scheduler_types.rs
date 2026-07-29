//! Shared scheduler output types.
//!
//! These types are used by both the scheduler backend and the web frontend so
//! that the JSON serialization format stays in sync. Keeping them in one place
//! prevents the "missing field" class of frontend errors when the scheduler
//! evolves.

use crate::{plan_types::ConstructionPlan, GameEcoParameters};
use serde::{Deserialize, Serialize};

/// A single step in the planned build order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepResult {
    pub action: Action,
    pub finish_time_seconds: f64,
    /// Number of builder units assigned to this step. For scheduler-generated
    /// steps this is typically one; it is preserved so the timeline can show
    /// actionable descriptions like "4 Engineers build X".
    pub builder_count: usize,
    pub economy: GameEcoParameters,
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
    pub final_eco: GameEcoParameters,
    pub steps: Vec<StepResult>,
}

/// Which economic direction a candidate score belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreCategory {
    MassIncome,
    Energy,
    BuildPower,
    TechT2,
    TechT3,
    Other,
}

/// Per-candidate score computation breakdown.
///
/// This explains how the final `score` in [`CandidateReasoning`] was derived so
/// the UI can show the user why one candidate outranked another.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CandidateScoreBreakdown {
    /// Eco scheduling score terms.
    Eco {
        category: ScoreCategory,
        /// Direction confidence used for this candidate (0–100).
        confidence: u8,
        /// Efficiency term: income delta per mass spent, engineer tier + 1, or 0.
        efficiency: f64,
        /// Simulated time to finish the action in seconds.
        time_seconds: f64,
        /// Time penalty subtracted from the base score.
        time_penalty: f64,
        /// Resource priority used as the multiplier (1–10).
        priority: u8,
        /// `priority / 5.0`, the actual multiplier applied to the base score.
        priority_multiplier: f64,
        /// Base score before the priority multiplier.
        base: f64,
    },
    /// Unit scheduling score terms.
    Unit {
        resulting_unit: faf_blueprints::UnitKind,
        /// Simulated time to finish the action in seconds.
        time_seconds: f64,
        /// Graph distance from the resulting unit to the target, if not the
        /// target itself.
        distance_to_target: Option<u32>,
    },
}

/// A candidate action that was considered for a scheduling step, together with
/// the score it received and an optional breakdown of how that score was
/// computed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateReasoning {
    pub action: Action,
    pub score: f64,
    #[serde(default)]
    pub breakdown: Option<CandidateScoreBreakdown>,
}

/// Confidence scores (0–100) for each economic direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DirectionScores {
    /// Confidence that the next step should increase energy income.
    pub energy: u8,
    /// Confidence that the next step should increase mass income.
    pub mass_income: u8,
    /// Confidence that the next step should increase build power (engineers).
    pub build_power: u8,
    /// Confidence that the next step should advance to T2 tech.
    pub tech_t2: u8,
    /// Confidence that the next step should advance to T3 tech.
    pub tech_t3: u8,
}

/// Priority multipliers (1–10) for the three resource categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PriorityTable {
    /// Priority for mass-income actions.
    pub mass: u8,
    /// Priority for energy-income actions.
    pub energy: u8,
    /// Priority for build-power (engineer) actions.
    pub build_power: u8,
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
    /// Direction confidence scores that led to this decision.
    pub direction_scores: DirectionScores,
    /// Priority weights used to scale the direction scores.
    pub priority_table: PriorityTable,
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
    #[error("the search was cancelled")]
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;
    use faf_blueprints::{TechLevel, UnitKind};

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
}
