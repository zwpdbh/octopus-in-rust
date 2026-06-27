//! Research CLI for FAF build-order simulation and optimization.
//!
//! ```text
//! faf-sim plan cybran monkeylord
//! faf-sim simulate cybran monkeylord
//! faf-sim simulate -s mcts:200 cybran monkeylord
//! ```
//!
//! `plan` emits an SVG image of the ACU-rooted plan graph showing the units
//! that must be built or upgraded to reach the goal, including both the
//! technology chain and the economic infrastructure. No timing or resource
//! simulation is performed; this is purely symbolic dependency planning.
//!
//! `simulate` runs the reactive simulator using the plan graph and a chosen
//! strategy to estimate a completion timeline.

use std::collections::HashMap;

use clap::Parser;
use faf_sim::{
    run_build_order_simulation, PlanEdgeKind, SimulationConfig, Strategy, UnitKind as SimUnitKind,
    Units as SimUnits,
};
use faf_units::DataIndex;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph_svg::{graph_to_svg, EdgeLabel, LegendItem, RenderOptions};

mod cmdline;
mod target;

use cmdline::{Cli, Command as CliCommand, FactionTarget};
use target::{Faction, ResearchTarget, UnitKind};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = Cli::parse();
    let index = load_index();
    let units = SimUnits::new(index.clone());

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

async fn run_simulate(units: &SimUnits, target: ResearchTarget, strategy: Strategy) {
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
    units: &SimUnits,
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
    let plan_graph = match units.plan_graph(&goal_kind) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Failed to build plan graph: {}", e);
            std::process::exit(1);
        }
    };

    let visual_graph = build_visual_graph(units, &plan_graph);

    let path = match output {
        Some(path) => path,
        None => {
            let file_name = format!(
                "faf-sim-plan-{}-{}.svg",
                target.faction.display_name().to_ascii_lowercase(),
                target
                    .unit
                    .display_name()
                    .to_ascii_lowercase()
                    .replace(' ', "-")
            );
            std::env::temp_dir().join(file_name)
        }
    };

    let options = RenderOptions {
        background_color: Some("#f0f4f8".to_string()),
        legend: vec![
            LegendItem::solid("build", "#555555"),
            LegendItem::dashed("upgrade", "#0066cc", "5,5"),
        ],
        ..RenderOptions::default()
    };
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

/// Visual edge style used to distinguish build and upgrade actions.
#[derive(Debug, Clone)]
struct VisualEdge {
    label: String,
    color: String,
    dash: Option<String>,
}

impl VisualEdge {
    fn for_kind(kind: PlanEdgeKind) -> Self {
        match kind {
            PlanEdgeKind::Build => Self {
                label: String::new(),
                color: "#555555".to_string(),
                dash: None,
            },
            PlanEdgeKind::Upgrade => Self {
                label: String::new(),
                color: "#0066cc".to_string(),
                dash: Some("5,5".to_string()),
            },
        }
    }
}

impl EdgeLabel for VisualEdge {
    fn label(&self) -> Option<String> {
        Some(self.label.clone())
    }

    fn color(&self) -> Option<String> {
        Some(self.color.clone())
    }

    fn dash_array(&self) -> Option<String> {
        self.dash.clone()
    }
}

/// Build a labelled petgraph from the ACU-rooted plan graph for rendering.
///
/// The returned graph preserves the structure of `plan_graph` but attaches
/// human-readable labels and edge styles.
fn build_visual_graph(
    units: &SimUnits,
    plan_graph: &DiGraph<SimUnitKind, PlanEdgeKind>,
) -> DiGraph<String, VisualEdge> {
    let mut graph = DiGraph::<String, VisualEdge>::new();
    let mut indices: HashMap<SimUnitKind, NodeIndex> = HashMap::new();

    for node in plan_graph.node_indices() {
        let kind = &plan_graph[node];
        indices.insert(kind.clone(), graph.add_node(node_label(units, kind)));
    }

    for edge in plan_graph.edge_references() {
        let from = indices[&plan_graph[edge.source()]];
        let to = indices[&plan_graph[edge.target()]];
        graph.add_edge(from, to, VisualEdge::for_kind(*edge.weight()));
    }

    graph
}

fn node_label(units: &SimUnits, kind: &SimUnitKind) -> String {
    use faf_sim::{TechLevel, UnitKind};
    let name = units.display_name(kind);
    match kind {
        UnitKind::Engineer(tl) | UnitKind::Factory(tl) | UnitKind::Mex(tl) | UnitKind::Pgen(tl) => {
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
