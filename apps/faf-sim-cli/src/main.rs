//! Research CLI for FAF build-order simulation and optimization.
//!
//! Targets are specified as a command, then optional strategy options, then a
//! faction subcommand, then a unit:
//!
//! ```text
//! faf-sim deps uef fatboy
//! faf-sim deps cybran monkeylord
//! faf-sim deps aeon nuke
//! faf-sim simulate cybran monkeylord
//! faf-sim simulate -s beam:20 cybran monkeylord
//! faf-sim plan -s greedy cybran monkeylord
//! ```
//!
//! Faction is a subcommand and each faction exposes only its own valid units as
//! a `ValueEnum`, so a typo like `faf-sim deps cybran monkeyloard` produces a
//! faction-scoped list of possible values.

use std::time::Duration;

use clap::Parser;
use faf_sim::{
    Capability, Command, Observation, Planner, PlannerActor, PlannerConfig, SimActor, Strategy,
    TechGraph,
};
use faf_units::DataIndex;
use tokio::sync::mpsc;

mod cmdline;
mod target;

use cmdline::{Cli, Command as CliCommand, FactionTarget};
use target::{Faction, ResearchTarget, UnitKind};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = Cli::parse();
    let index = load_index();
    let graph = TechGraph::new(&index);

    match cli.command {
        CliCommand::Deps(args) => {
            let (faction, unit) = resolve_faction_target(args.target);
            let target = resolve_target(faction, unit);
            run_deps(&graph, target, &args.stop_at);
        }
        CliCommand::Simulate(args) => {
            let (faction, unit) = resolve_faction_target(args.target);
            let target = resolve_target(faction, unit);
            run_simulate(&index, target, args.strategy).await;
        }
        CliCommand::Plan(args) => {
            let (faction, unit) = resolve_faction_target(args.target);
            let target = resolve_target(faction, unit);
            run_plan(&index, &graph, target, args.strategy);
        }
    }
}

fn load_index() -> DataIndex {
    let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
    serde_json::from_str(json).expect("embedded FAF unit index should parse")
}

/// Convert a faction subcommand into the internal `(Faction, UnitKind)` pair.
fn resolve_faction_target(target: FactionTarget) -> (Faction, UnitKind) {
    match target {
        FactionTarget::Uef(args) => (Faction::Uef, args.unit.into()),
        FactionTarget::Cybran(args) => (Faction::Cybran, args.unit.into()),
        FactionTarget::Aeon(args) => (Faction::Aeon, args.unit.into()),
        FactionTarget::Seraphim(args) => (Faction::Seraphim, args.unit.into()),
    }
}

fn resolve_target(faction: Faction, unit: UnitKind) -> ResearchTarget {
    let target = ResearchTarget { faction, unit };
    if let Err(e) = target.validate() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
    target
}

fn run_deps(graph: &TechGraph, target: ResearchTarget, stop_at: &[String]) {
    let blueprint_id = target.blueprint_id();
    let unit = match graph.index().find_unit(blueprint_id) {
        Some(u) => u,
        None => {
            eprintln!("Blueprint id not found in index: {}", blueprint_id);
            std::process::exit(1);
        }
    };

    println!(
        "Target: {} — {} / {} [{}]",
        target.display_name(),
        unit.display_name(),
        unit.name_zh().unwrap_or("?"),
        unit.tech_level().unwrap_or("?")
    );

    println!("\nDirect builders:");
    match graph.builders_for(blueprint_id) {
        Ok(builders) if builders.is_empty() => println!("  (none)"),
        Ok(builders) => {
            for b in builders {
                println!(
                    "  {} — {} / {} [{}]",
                    b.id,
                    b.display_name(),
                    b.name_zh().unwrap_or("?"),
                    b.tech_level().unwrap_or("?")
                );
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }

    println!("\nTransitive prerequisites:");
    let prereqs = if stop_at.is_empty() {
        graph.all_prerequisites_default(blueprint_id)
    } else {
        let refs: Vec<&str> = stop_at.iter().map(|s| s.as_str()).collect();
        graph.all_prerequisites(blueprint_id, &refs)
    };

    match prereqs {
        Ok(units) => {
            if units.is_empty() {
                println!("  (none)");
            } else {
                for p in units {
                    println!(
                        "  {} — {} / {} [{}]",
                        p.id,
                        p.display_name(),
                        p.name_zh().unwrap_or("?"),
                        p.tech_level().unwrap_or("?")
                    );
                }
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

async fn run_simulate(index: &DataIndex, target: ResearchTarget, strategy: Strategy) {
    let blueprint_id = target.blueprint_id();
    let target_unit = index
        .find_unit(blueprint_id)
        .expect("target blueprint must exist in index");

    println!("Strategy: {}", strategy);
    println!("Simulate target: {}", target.display_name());

    let starting_unit = index
        .find_unit(match target.faction {
            Faction::Uef => "UEL0001",
            Faction::Cybran => "URL0001",
            Faction::Aeon => "UAL0001",
            Faction::Seraphim => "XSL0001",
        })
        .expect("ACU should exists in index");

    // Pause tokio's clock so the actor loop can be driven forward
    // deterministically without waiting for real wall-clock time.
    tokio::time::pause();

    let sim_dt = 10.0;
    let max_sim_time = 8.0 * 60.0 * 60.0; // 8 hours of in-game time
    let max_ticks = (max_sim_time / sim_dt) as usize + 1;

    let (obs_tx, obs_rx) = mpsc::channel::<Observation>(64);
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);

    let sim = SimActor::new(
        &[starting_unit],
        index.clone(),
        Some(target_unit),
        sim_dt,
        obs_tx,
        cmd_rx,
    );
    let sim_handle = tokio::spawn(sim.run());

    let planner = build_reactive_planner(strategy);
    let planner_actor =
        PlannerActor::new(planner, index.clone(), target_unit.clone(), obs_rx, cmd_tx);
    let planner_handle = tokio::spawn(planner_actor.run());

    // Drive the simulation timer forward until the goal is reached or we hit
    // the safety cap.
    let tick = Duration::from_secs_f64(sim_dt);
    let mut ticks = 0;
    while !sim_handle.is_finished() {
        if ticks >= max_ticks {
            sim_handle.abort();
            planner_handle.abort();
            eprintln!(
                "Simulation did not reach the goal within {:.1} hours of in-game time.",
                max_sim_time / 3600.0
            );
            std::process::exit(1);
        }
        tokio::time::advance(tick).await;
        ticks += 1;
    }

    let final_state = match sim_handle.await {
        Ok(Ok(state)) => state,
        Ok(Err(e)) => {
            eprintln!("Simulation error: {}", e);
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Simulation task panicked: {}", e);
            std::process::exit(1);
        }
    };

    // The planner actor exits once the observation channel is closed.
    let _ = planner_handle.await;

    if final_state.time > max_sim_time {
        eprintln!(
            "Simulation did not reach the goal within {:.1} hours of in-game time.",
            max_sim_time / 3600.0
        );
        std::process::exit(1);
    }

    println!(
        "\nGoal completed at {} ({:.1}m)",
        format_time(final_state.time),
        final_state.time / 60.0
    );
    println!("\nTimeline:");
    println!("{:>12}  {}", "Time", "Unit");
    println!("{:>12}  {}", "------------", "----");
    for event in &final_state.events {
        println!(
            "{:>12}  {} ({})",
            format_time(event.time),
            event.unit_name,
            event.unit_id
        );
    }
}

/// Build a reactive planner for the actor loop from the CLI strategy.
fn build_reactive_planner(strategy: Strategy) -> Planner {
    let config = match strategy {
        Strategy::Greedy => PlannerConfig {
            dt: 1.0,
            max_depth: 400,
            ..PlannerConfig::default()
        },
        Strategy::Beam { .. } => PlannerConfig {
            dt: 10.0,
            max_depth: 400,
            ..PlannerConfig::default()
        },
    };
    Planner::with_config(strategy, config)
}

/// Format seconds as "Mm Ss" with one decimal place on seconds.
fn format_time(seconds: f64) -> String {
    let minutes = (seconds / 60.0).floor();
    let secs = seconds - minutes * 60.0;
    format!("{:.0}m {:.1}s", minutes, secs)
}

fn run_plan(index: &DataIndex, graph: &TechGraph, target: ResearchTarget, strategy: Strategy) {
    println!(
        "Plan target: {} strategy: {}",
        target.display_name(),
        strategy
    );

    match strategy {
        Strategy::Greedy => {}
        Strategy::Beam { beam_width } => {
            println!("Beam width: {}", beam_width);
        }
    }

    let blueprint_id = target.blueprint_id();

    match graph.prerequisite_chain(blueprint_id, Capability::ACU) {
        Ok(chain) => {
            println!("\nSymbolic tech chain:");
            for (i, (cap, unit_id)) in chain.iter().enumerate() {
                let name = index
                    .find_unit(unit_id)
                    .map(|u| u.display_name())
                    .unwrap_or_else(|| unit_id.clone());
                println!("{:>2}. {} → build {} ({})", i + 1, cap, name, unit_id);
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
