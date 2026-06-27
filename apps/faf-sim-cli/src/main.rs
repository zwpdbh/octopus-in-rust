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

use std::collections::{HashMap, HashSet};

use clap::Parser;
use faf_sim::{
    run_build_order_simulation, GraphState, NodeId, PlanEdgeKind, PlanGraph, Planner,
    SimulationConfig, Strategy, UnitKind as SimUnitKind, Units as SimUnits,
};
use faf_sim::planner::mcts::train::{
    load_model, save_model, train_policy, TrainConfig,
};
use faf_units::DataIndex;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph_svg::{graph_to_svg, EdgeLabel, LegendItem, NodeLabel, RenderOptions};

mod cmdline;
mod target;

use cmdline::{Cli, Command as CliCommand, FactionTarget, TrainArgs};
use target::{Faction, ResearchTarget, UnitKind};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = Cli::parse();
    let index = load_index();
    let units = SimUnits::new(index.clone());

    match cli.command {
        CliCommand::Plan(args) => {
            let (faction, unit) = resolve_faction_target(&args.target);
            let target = resolve_target(faction, unit);
            run_plan(&units, &index, target, args.output);
        }
        CliCommand::Train(args) => {
            let (faction, unit) = resolve_faction_target(&args.target);
            let target = resolve_target(faction, unit);
            run_train(&units, target, args);
        }
        CliCommand::Simulate(args) => {
            let (faction, unit) = resolve_faction_target(&args.target);
            let target = resolve_target(faction, unit);
            run_simulate(&units, target, args.strategy, args.output).await;
        }
    }
}

fn load_index() -> DataIndex {
    let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
    serde_json::from_str(json).expect("embedded FAF unit index should parse")
}

/// Convert a faction subcommand into the internal `(Faction, UnitKind)` pair.
fn resolve_faction_target(target: &FactionTarget) -> (Faction, UnitKind) {
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

fn model_path(target: &ResearchTarget) -> std::path::PathBuf {
    let file_name = format!(
        "mlp-{}-{}",
        target.faction.display_name().to_ascii_lowercase(),
        target
            .unit
            .display_name()
            .to_ascii_lowercase()
            .replace(' ', "-")
    );
    std::path::PathBuf::from("data/models").join(file_name)
}

fn run_train(units: &SimUnits, target: ResearchTarget, args: TrainArgs) {
    let goal_kind = target.to_sim_unit_kind();
    units
        .def(&goal_kind)
        .expect("target blueprint must exist in index");

    println!(
        "Training MLP for {} {}",
        target.faction.display_name(),
        target.unit.display_name()
    );

    let config = TrainConfig {
        episodes: args.episodes,
        max_steps: args.max_steps,
        ..Default::default()
    };
    let (model, stats) = train_policy(units, &goal_kind, config);

    println!(
        "Training complete: {}/{} episodes reached the goal",
        stats.goal_reaches, args.episodes
    );
    if let Some(&best) = stats.completion_times.iter().min_by(|a, b| a.partial_cmp(b).unwrap()) {
        println!("Best completion time: {}", format_time(best));
    }

    let path = model_path(&target);
    save_model(&model, &path).expect("save trained model");
    println!("Model saved to {}", path.display());
}

async fn run_simulate(
    units: &SimUnits,
    target: ResearchTarget,
    strategy: Strategy,
    output: Option<std::path::PathBuf>,
) {
    use faf_sim::PlannerConfig;

    let goal_kind = target.to_sim_unit_kind();
    units
        .def(&goal_kind)
        .expect("target blueprint must exist in index");

    println!("Strategy: {}", strategy);
    println!("Simulate target: {}", target.display_name());

    let path = model_path(&target);
    let model_file = path.with_extension("mpk");
    let planner = if model_file.exists() {
        println!("Loading trained model from {}", model_file.display());
        let model = load_model(&path).expect("load trained model");
        Planner::with_value_net(strategy, PlannerConfig::default(), model)
    } else {
        println!("No trained model found; using random initialization");
        Planner::reactive(strategy)
    };

    let config = SimulationConfig {
        planner,
        sim_dt: 10.0,
        max_sim_time: 8.0 * 60.0 * 60.0,
    };
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

    let visual_graph = build_simulation_visual_graph(units, &result.final_state);

    let svg_path = match output {
        Some(path) => path,
        None => {
            let file_name = format!(
                "faf-sim-simulate-{}-{}.svg",
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
        node_width: 180.0,
        node_height: 60.0,
        font_size: 11.0,
        legend: vec![LegendItem::solid("build power contribution", "#555555")],
        ..RenderOptions::default()
    };
    if let Err(e) = graph_to_svg(&visual_graph, &svg_path, &options) {
        eprintln!("Failed to render simulation build order to SVG: {}", e);
        std::process::exit(1);
    }

    println!("\nBuild-order diagram written to:");
    println!("  {}", svg_path.display());
    if let Ok(absolute) = std::fs::canonicalize(&svg_path) {
        println!("  file://{}", absolute.display());
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
    plan_graph: &PlanGraph,
) -> petgraph::graph::DiGraph<String, VisualEdge> {
    let inner = plan_graph.graph();
    let mut graph = petgraph::graph::DiGraph::<String, VisualEdge>::new();
    let mut indices: HashMap<SimUnitKind, NodeIndex> = HashMap::new();

    for node in inner.node_indices() {
        let kind = &inner[node];
        indices.insert(kind.clone(), graph.add_node(node_label(units, kind)));
    }

    for edge in inner.edge_references() {
        let from = indices[&inner[edge.source()]];
        let to = indices[&inner[edge.target()]];
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

/// Visual node representing one built-unit slot from the simulation.
#[derive(Debug, Clone)]
struct SimVisualNode {
    label: String,
    color: String,
}

impl NodeLabel for SimVisualNode {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn color(&self) -> Option<String> {
        Some(self.color.clone())
    }
}

/// Builder-assignment edge in the simulation build-order diagram.
#[derive(Debug, Clone)]
struct SimVisualEdge;

impl EdgeLabel for SimVisualEdge {
    fn color(&self) -> Option<String> {
        Some("#555555".to_string())
    }
}

/// Build a labelled petgraph from the simulation state for rendering.
///
/// The graph follows the model in `crates/faf-sim/doc/model.md`: one node per
/// built-unit slot and directed edges `builder -> target` for every build-power
/// contribution. Upgraded slots keep their original node and sequence number;
/// their label shows the final unit and the upgrade time.
fn build_simulation_visual_graph(
    units: &SimUnits,
    state: &GraphState,
) -> petgraph::graph::DiGraph<SimVisualNode, SimVisualEdge> {
    let mut graph = petgraph::graph::DiGraph::<SimVisualNode, SimVisualEdge>::new();

    // Map from build-graph NodeId to the single visual node representing that slot.
    let mut node_indices: HashMap<NodeId, NodeIndex> = HashMap::new();

    // Collect every finished slot with the time it was first created.
    // Starting units exist at time 0. Every other slot first appears as a
    // construction event; upgrades reuse the same slot and do not get a new
    // sequence number.
    let mut first_event_time: HashMap<NodeId, f64> = HashMap::new();
    let mut original_unit: HashMap<NodeId, SimUnitKind> = HashMap::new();

    for node in state.graph.graph.node_weights() {
        if node.finish_time() == Some(0.0) {
            first_event_time.insert(node.id, 0.0);
            original_unit.insert(node.id, node.unit_id.clone());
        }
    }

    let mut seen_slots: HashSet<NodeId> = HashSet::new();
    for event in &state.events {
        if seen_slots.insert(event.node_id) {
            first_event_time.insert(event.node_id, event.time);
            original_unit.insert(event.node_id, event.unit_id.clone());
        }
    }

    // Sort slots by creation time and assign sequence numbers.
    let mut slots: Vec<NodeId> = first_event_time.keys().copied().collect();
    slots.sort_by(|a, b| {
        first_event_time[a]
            .partial_cmp(&first_event_time[b])
            .unwrap()
            .then(a.cmp(b))
    });

    for (seq, slot_id) in slots.iter().enumerate() {
        let slot = &state.graph.graph[slot_id.0];
        let created_at = first_event_time[slot_id];
        let is_starting = created_at == 0.0;
        let is_upgraded = slot.from_unit_id().is_some();

        let color = if is_starting {
            "#d4edda".to_string()
        } else if is_upgraded {
            "#cce5ff".to_string()
        } else {
            "#f8f9fa".to_string()
        };

        let finish_time = slot.finish_time().unwrap_or(created_at);
        let name = node_label(units, &slot.unit_id);

        let mut label = if is_starting {
            format!("[{}] {}\n(start)", seq + 1, name)
        } else if is_upgraded {
            format!(
                "[{}] {}\nupgraded {}",
                seq + 1,
                name,
                format_time(finish_time)
            )
        } else {
            format!("[{}] {}\n{}", seq + 1, name, format_time(finish_time))
        };

        if is_upgraded {
            if let Some(original) = original_unit.get(slot_id) {
                label.push_str(&format!("\n(was {})", node_label(units, original)));
            }
        }

        let idx = graph.add_node(SimVisualNode { label, color });
        node_indices.insert(*slot_id, idx);
    }

    // Builder assignment edges: one edge per build-power contribution.
    for edge in state.graph.graph.edge_references() {
        let builder_id = NodeId(edge.source());
        let target_id = NodeId(edge.target());

        if edge.weight().finish_time.is_nan() {
            // The project did not complete; skip it.
            continue;
        }

        if let (Some(&from), Some(&to)) =
            (node_indices.get(&builder_id), node_indices.get(&target_id))
        {
            graph.add_edge(from, to, SimVisualEdge);
        }
    }

    graph
}
