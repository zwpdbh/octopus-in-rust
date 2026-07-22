//! Optional per-step trace output for debugging the scheduler lifecycle.
//!
//! Register [`SchedulerTracePlugin`] after a scheduling-mode plugin to print a
//! summary of every Observe/Decide/Apply cycle to stdout.

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;

use crate::plugins::apply::StepReasoningLog;
use crate::plugins::eco::observe::Observation;
use crate::plugins::lifecycle::SchedulerSet;
use crate::resources::StepLog;

/// Plugin that prints a debug trace of each scheduling cycle.
pub struct SchedulerTracePlugin;

impl Plugin for SchedulerTracePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, trace_step_system.in_set(SchedulerSet::Apply));
    }
}

fn trace_step_system(
    step_log: Res<StepLog>,
    reasoning_log: Res<StepReasoningLog>,
    observation: Res<Observation>,
) {
    let Some(step) = step_log.0.last() else {
        return;
    };
    let Some(reasoning) = reasoning_log.0.last() else {
        return;
    };

    let eco = &step.economy;
    eprintln!("\n=== Scheduler cycle #{} ===", reasoning_log.0.len());
    eprintln!("Observation: {:?}", &*observation);
    eprintln!("DirectionScores: {:?}", reasoning.direction_scores);
    eprintln!("PriorityTable: {:?}", reasoning.priority_table);
    eprintln!("Chosen action: {:?}", reasoning.chosen);
    eprintln!("Top candidates:");
    for candidate in reasoning.top_candidates.iter().take(5) {
        let marker = if candidate.action == reasoning.chosen {
            " <- chosen"
        } else {
            ""
        };
        eprintln!(
            "  {:>8.2}  {:?}{}",
            candidate.score, candidate.action, marker
        );
    }
    eprintln!(
        "Result economy: mass {:>5.1}/s | energy {:>5.1}/s | mass_storage {:>6.0}/{:<6.0} | energy_storage {:>6.0}/{:<6.0} | time {:.1}s",
        eco.production_per_second_mass.value(),
        eco.production_per_second_energy.value(),
        eco.mass_storage.value(),
        eco.mass_storage_cap.value(),
        eco.energy_storage.value(),
        eco.energy_storage_cap.value(),
        eco.time.value(),
    );
}
