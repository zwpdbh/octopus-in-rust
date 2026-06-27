//! Research CLI for FAF build-order simulation and optimization.
//!
//! ```text
//! faf-sim plan cybran monkeylord
//! faf-sim simulate cybran monkeylord
//! faf-sim simulate -s beam:20 cybran monkeylord
//! ```
//!
//! `plan` emits an SVG image of the dependency graph showing the units that
//! must be built (or upgraded) to reach the goal. No timing or resource
//! simulation is performed; this is purely symbolic dependency planning.
//!
//! `simulate` runs the reactive simulator using the plan graph and a chosen
//! strategy to estimate a completion timeline.

use std::collections::HashMap;

use clap::Parser;
use faf_sim::{run_build_order_simulation, DependencyNode, Fact, SimulationConfig, Strategy};
use faf_units::DataIndex;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph_svg::{graph_to_svg, RenderOptions};

mod cmdline;
mod target;

use cmdline::{Cli, Command as CliCommand, FactionTarget};
use target::{Faction, ResearchTarget, UnitKind};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = Cli::parse();
    let index = load_index();
    let units = faf_sim::Units::new(index.clone());

    match cli.command {
        CliCommand::Plan(args) => {
            let (faction, unit) = resolve_faction_target(args.target);
            let target = resolve_target(faction, unit);
            run_plan(&units, &index, target, args.output);
        }
        CliCommand::Simulate(args) => {
            let (faction, unit) = resolve_faction_target(args.target);
            let target = resolve_target(faction, unit);
            run_simulate(&units, target, args.strategy).await;
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

async fn run_simulate(units: &faf_sim::Units, target: ResearchTarget, strategy: Strategy) {
    let goal_kind = target.to_sim_unit_kind();
    units
        .def(&goal_kind)
        .expect("target blueprint must exist in index");

    println!("Strategy: {}", strategy);
    println!("Simulate target: {}", target.display_name());

    let config = SimulationConfig::for_strategy(strategy);
    let result = match run_build_order_simulation(units.clone(), goal_kind.clone(), config).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Simulation error: {}", e);
            std::process::exit(1);
        }
    };

    println!(
        "\nGoal completed at {} ({:.1}m)",
        format_time(result.final_state.time),
        result.final_state.time / 60.0
    );
    println!("\nTimeline:");
    println!("{:>12}  Unit", "Time");
    println!("{:>12}  ----", "------------");
    for event in &result.final_state.events {
        println!(
            "{:>12}  {} ({:?})",
            format_time(event.time),
            units.display_name(&event.unit_id),
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

fn run_plan(
    units: &faf_sim::Units,
    index: &DataIndex,
    target: ResearchTarget,
    output: Option<std::path::PathBuf>,
) {
    let blueprint_id = target.blueprint_id();
    if index.find_unit(blueprint_id).is_none() {
        eprintln!("Blueprint id not found in index: {}", blueprint_id);
        std::process::exit(1);
    }

    let goal_kind = target.to_sim_unit_kind();
    let dep_graph = match units.dependency_graph(&goal_kind) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Failed to build dependency graph: {}", e);
            std::process::exit(1);
        }
    };

    let visual_graph = build_visual_graph(units, &dep_graph.root);

    let path = match output {
        Some(path) => path,
        None => {
            let file_name = format!(
                "faf-sim-plan-{}-{}.svg",
                target.faction.display_name().to_ascii_lowercase(),
                target.unit.display_name().to_ascii_lowercase().replace(' ', "-")
            );
            std::env::temp_dir().join(file_name)
        }
    };

    let options = RenderOptions::default();
    if let Err(e) = graph_to_svg(&visual_graph, &path, &options) {
        eprintln!("Failed to render plan to SVG: {}", e);
        std::process::exit(1);
    }

    println!("Build plan written to:");
    println!("  {}", path.display());
    if let Ok(absolute) = std::fs::canonicalize(&path) {
        println!("  file://{}", absolute.display());
    }
}

/// Build a directed graph from the dependency tree, collapsing duplicate unit
/// kinds into a single node.
///
/// Edges point from a unit toward its prerequisites (e.g. `goal -> factory_t3
/// -> factory_t1 -> commander`) so that the rendered image reads top-down from
/// the goal to the starting units.
fn build_visual_graph(
    units: &faf_sim::Units,
    root: &DependencyNode,
) -> DiGraph<String, ()> {
    let mut graph = DiGraph::<String, ()>::new();
    let mut indices: HashMap<faf_sim::UnitKind, NodeIndex> = HashMap::new();

    fn ensure_node(
        graph: &mut DiGraph<String, ()>,
        units: &faf_sim::Units,
        kind: &faf_sim::UnitKind,
        indices: &mut HashMap<faf_sim::UnitKind, NodeIndex>,
    ) -> NodeIndex {
        *indices
            .entry(kind.clone())
            .or_insert_with(|| graph.add_node(node_label(units, kind)))
    }

    fn node_label(units: &faf_sim::Units, kind: &faf_sim::UnitKind) -> String {
        use faf_sim::{TechLevel, UnitKind};
        let name = units.display_name(kind);
        match kind {
            UnitKind::Engineer(tl)
            | UnitKind::Factory(tl)
            | UnitKind::Mex(tl)
            | UnitKind::Pgen(tl) => {
                let tier = match tl {
                    TechLevel::T1 => 1,
                    TechLevel::T2 => 2,
                    TechLevel::T3 => 3,
                    TechLevel::T4 => 4,
                };
                format!("{} (T{})", name, tier)
            }
            UnitKind::Commander | UnitKind::Unique(_) => name.to_string(),
        }
    }

    fn walk(
        graph: &mut DiGraph<String, ()>,
        units: &faf_sim::Units,
        node: &DependencyNode,
        indices: &mut HashMap<faf_sim::UnitKind, NodeIndex>,
    ) {
        let Fact::Have(target_kind) = &node.goal;
        let target_idx = ensure_node(graph, units, target_kind, indices);

        for sub in &node.subgoals {
            let Fact::Have(source_kind) = &sub.goal;
            let source_idx = ensure_node(graph, units, source_kind, indices);
            // Add edge only once; petgraph allows parallel edges but the
            // visualiser is clearer without duplicates.
            if graph.find_edge(target_idx, source_idx).is_none() {
                graph.add_edge(target_idx, source_idx, ());
            }
            walk(graph, units, sub, indices);
        }
    }

    walk(&mut graph, units, root, &mut indices);
    graph
}
