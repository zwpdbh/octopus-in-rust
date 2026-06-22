//! Research CLI for FAF build-order simulation and optimization.
//!
//! Targets are specified as a faction flag plus a unit name:
//!
//! ```text
//! faf-sim deps -u fatboy
//! faf-sim deps -c monkeylord
//! faf-sim deps -a nuke
//! faf-sim deps -s arty
//! ```

use std::str::FromStr;

use clap::{Parser, Subcommand};
use faf_sim::{build_planner, Capability, Strategy, TechGraph};
use faf_units::DataIndex;

mod target;
use target::{Faction, ResearchTarget, UnitKind};

#[derive(Parser)]
#[command(name = "faf-sim")]
#[command(about = "Research CLI for FAF build-order simulation and optimization")]
#[command(after_help = ResearchTarget::help_text())]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Parser)]
#[group(required = true, multiple = false)]
struct FactionArgs {
    /// UEF faction.
    #[arg(short = 'u', long)]
    uef: bool,
    /// Cybran faction.
    #[arg(short = 'c', long)]
    cybran: bool,
    /// Aeon faction.
    #[arg(short = 'a', long)]
    aeon: bool,
    /// Seraphim faction.
    #[arg(short = 's', long)]
    seraphim: bool,
}

impl FactionArgs {
    fn to_faction(&self) -> Faction {
        // Clap's group guarantees exactly one is true.
        if self.uef {
            Faction::Uef
        } else if self.cybran {
            Faction::Cybran
        } else if self.aeon {
            Faction::Aeon
        } else {
            Faction::Seraphim
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Show the dependency graph for a target unit.
    Deps {
        #[command(flatten)]
        faction: FactionArgs,
        /// Unit name, e.g. `fatboy`, `monkeylord`, `nuke`, `arty`.
        unit: UnitKind,
        /// Stop expanding prerequisites at these unit ids (default: commanders).
        #[arg(long, value_delimiter = ',')]
        stop_at: Vec<String>,
    },
    /// Simulate a build order and print timing/resource trace.
    Simulate {
        #[command(flatten)]
        faction: FactionArgs,
        /// Unit name, e.g. `fatboy`.
        unit: UnitKind,
        /// Planner strategy.
        #[arg(long, default_value = "greedy")]
        strategy: String,
        /// Path to a JSON build-order file.
        #[arg(short, long)]
        order: Option<String>,
    },
    /// Generate a build order for a target unit.
    Plan {
        #[command(flatten)]
        faction: FactionArgs,
        /// Unit name, e.g. `fatboy`.
        unit: UnitKind,
        /// Planner strategy.
        #[arg(long, default_value = "greedy")]
        strategy: String,
    },
}

fn main() {
    let cli = Cli::parse();
    let index = load_index();
    let graph = TechGraph::new(&index);

    match cli.command {
        Commands::Deps {
            faction,
            unit,
            stop_at,
        } => {
            let target = resolve_target(faction, unit);
            run_deps(&graph, target, &stop_at);
        }
        Commands::Simulate {
            faction,
            unit,
            strategy,
            order,
        } => {
            let target = resolve_target(faction, unit);
            let strategy = parse_strategy(&strategy);
            run_simulate(&index, &graph, target, order.as_deref(), strategy);
        }
        Commands::Plan {
            faction,
            unit,
            strategy,
        } => {
            let target = resolve_target(faction, unit);
            let strategy = parse_strategy(&strategy);
            run_plan(&index, &graph, target, strategy);
        }
    }
}

fn load_index() -> DataIndex {
    let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
    serde_json::from_str(json).expect("embedded FAF unit index should parse")
}

fn resolve_target(faction: FactionArgs, unit: UnitKind) -> ResearchTarget {
    let target = ResearchTarget {
        faction: faction.to_faction(),
        unit,
    };
    if let Err(e) = target.validate() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
    target
}

fn parse_strategy(raw: &str) -> Strategy {
    match Strategy::from_str(raw) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
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

fn run_simulate(
    index: &DataIndex,
    graph: &TechGraph,
    target: ResearchTarget,
    order: Option<&str>,
    strategy: Strategy,
) {
    let blueprint_id = target.blueprint_id();
    let target_unit = index
        .find_unit(blueprint_id)
        .expect("target blueprint must exist in index");

    println!("Strategy: {}", strategy);
    println!("Simulate target: {}", target.display_name());

    if let Some(path) = order {
        println!("Build order file: {}", path);
        println!("(Custom build order files are not yet supported.)");
        return;
    }

    let starting_unit = index
        .find_unit(match target.faction {
            Faction::Uef => "UEL0001",
            Faction::Cybran => "URL0001",
            Faction::Aeon => "UAL0001",
            Faction::Seraphim => "XSL0001",
        })
        .expect("ACU exists in index");

    let planner = match build_planner(strategy) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let result = planner
        .plan(index, graph, &[starting_unit], target_unit)
        .unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        });

    println!(
        "\nGoal completed at {} ({:.1}m)",
        format_time(result.completion_time),
        result.completion_time / 60.0
    );
    println!("\nTimeline:");
    println!("{:>12}  {}", "Time", "Unit");
    println!("{:>12}  {}", "------------", "----");
    for event in &result.events {
        println!(
            "{:>12}  {} ({})",
            format_time(event.time),
            event.unit_name,
            event.unit_id
        );
    }
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

    if let Err(e) = build_planner(strategy) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
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
