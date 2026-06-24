//! Benchmark example: compare planner strategies across FAF targets.
//!
//! Run with:
//!
//! ```text
//! cargo run -p faf-sim --example bench_planners --release
//! ```
//!
//! The example reports in-game completion time and wall-clock planning time for
//! each (faction, target, strategy) combination. Use it to quantify improvements
//! when optimizing the beam search.

use std::time::Instant;

use faf_sim::{build_planner, GraphState, Strategy};
use faf_units::DataIndex;

#[derive(Debug, Clone, Copy)]
struct Target {
    acu_id: &'static str,
    goal_id: &'static str,
    name: &'static str,
}

const TARGETS: &[Target] = &[
    Target {
        acu_id: "UEL0001",
        goal_id: "UEL0401",
        name: "UEF Fatboy",
    },
    Target {
        acu_id: "URL0001",
        goal_id: "URL0402",
        name: "Cybran Monkeylord",
    },
    Target {
        acu_id: "UAL0001",
        goal_id: "UAL0401",
        name: "Aeon Galactic Colossus",
    },
    Target {
        acu_id: "XSL0001",
        goal_id: "XSL0401",
        name: "Seraphim Ythotha",
    },
];

const STRATEGIES: &[Strategy] = &[Strategy::Greedy, Strategy::Beam { beam_width: 50 }];

fn load_index() -> DataIndex {
    let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
    serde_json::from_str(json).expect("embedded index should parse")
}

fn format_time(seconds: f64) -> String {
    let minutes = (seconds / 60.0).floor();
    let secs = seconds - minutes * 60.0;
    format!("{:.0}m {:.1}s", minutes, secs)
}

fn main() {
    let index = load_index();

    println!(
        "{:<24} {:<12} {:>14} {:>14}",
        "Target", "Strategy", "Game Time", "Plan Time"
    );
    println!("{}", "-".repeat(66));

    for target in TARGETS {
        let acu = index
            .find_unit(target.acu_id)
            .unwrap_or_else(|| panic!("ACU {} not found", target.acu_id));
        let goal = index
            .find_unit(target.goal_id)
            .unwrap_or_else(|| panic!("goal {} not found", target.goal_id));

        for strategy in STRATEGIES {
            let planner = build_planner(*strategy).expect("valid strategy");

            let start = Instant::now();
            let initial = GraphState::new(&[acu]);
            let result = planner.plan(&index, initial, goal);
            let elapsed = start.elapsed();

            match result {
                Ok(plan) => {
                    let events = plan.events.len();
                    println!(
                        "{:<24} {:<12} {:>14} {:>13.2?}  ({} events)",
                        target.name,
                        strategy.display_name(),
                        format_time(plan.completion_time),
                        elapsed,
                        events
                    );
                }
                Err(e) => {
                    println!(
                        "{:<24} {:<12} {:>14} {:>13.2?}  ERROR: {}",
                        target.name,
                        strategy.display_name(),
                        "—",
                        elapsed,
                        e
                    );
                }
            }
        }
        println!();
    }

    // Sanity check: beam search should beat the greedy baseline on the
    // targets used in the existing unit tests.
    let acu = index.find_unit("URL0001").expect("ACU exists");
    let goal = index.find_unit("URL0402").expect("Monkeylord exists");

    let greedy = build_planner(Strategy::Greedy)
        .unwrap()
        .plan(&index, GraphState::new(&[acu]), goal)
        .unwrap();
    let beam = build_planner(Strategy::Beam { beam_width: 50 })
        .unwrap()
        .plan(&index, GraphState::new(&[acu]), goal)
        .unwrap();

    println!(
        "Sanity check: beam ({}) / greedy ({}) for Monkeylord",
        format_time(beam.completion_time),
        format_time(greedy.completion_time)
    );
    assert!(
        beam.completion_time < greedy.completion_time,
        "beam should beat greedy on Monkeylord"
    );
}
