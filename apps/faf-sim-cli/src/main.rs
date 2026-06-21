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

use clap::{Parser, Subcommand};
use faf_sim::BuildGraph;
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
        #[arg(short, long, default_value = "greedy")]
        strategy: String,
    },
}

fn main() {
    let cli = Cli::parse();
    let index = load_index();
    let graph = BuildGraph::new(&index);

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
            order,
        } => {
            let target = resolve_target(faction, unit);
            run_simulate(target, order.as_deref());
        }
        Commands::Plan {
            faction,
            unit,
            strategy,
        } => {
            let target = resolve_target(faction, unit);
            run_plan(target, &strategy);
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

fn run_deps(graph: &BuildGraph, target: ResearchTarget, stop_at: &[String]) {
    let blueprint_id = target.blueprint_id();
    let unit = match graph.unit(blueprint_id) {
        Some(u) => u,
        None => {
            eprintln!("Blueprint id not found in index: {}", blueprint_id);
            std::process::exit(1);
        }
    };

    println!(
        "Target: {} — {} / {} [{}]",
        target.display_name(),
        unit.name().unwrap_or("?"),
        unit.name_zh().unwrap_or("?"),
        unit.tech_level().unwrap_or("?")
    );

    println!("\nDirect builders:");
    let builders = graph.builders_for(blueprint_id);
    if builders.is_empty() {
        println!("  (none)");
    } else {
        for b in builders {
            println!(
                "  {} — {} / {} [{}]",
                b.id,
                b.name().unwrap_or("?"),
                b.name_zh().unwrap_or("?"),
                b.tech_level().unwrap_or("?")
            );
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
                        p.name().unwrap_or("?"),
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

fn run_simulate(target: ResearchTarget, order: Option<&str>) {
    println!("Simulate target: {}", target.display_name());
    if let Some(path) = order {
        println!("Build order file: {}", path);
    } else {
        println!("No build order file provided; using placeholder.");
    }
    println!("(Simulator not yet implemented.)");
}

fn run_plan(target: ResearchTarget, strategy: &str) {
    println!(
        "Plan target: {} strategy: {}",
        target.display_name(),
        strategy
    );
    println!("(Planner not yet implemented.)");
}
