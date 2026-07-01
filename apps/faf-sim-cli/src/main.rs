//! Research CLI for FAF build-order simulation and optimization.
//!
//! ```text
//! faf-sim plan
//! faf-sim simulate cybran monkeylord
//! faf-sim simulate -s mcts:200 cybran monkeylord
//! ```
//!
//! `plan` emits an SVG image of the universal ACU-rooted plan graph showing the
//! build/upgrade relationships between all units. No timing or resource
//! simulation is performed; this is purely symbolic dependency planning.
//!
//! `simulate` runs the reactive simulator using the plan graph and a chosen
//! strategy to estimate a completion timeline.

use std::collections::{HashMap, HashSet};
use std::io::IsTerminal;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::Parser;
use faf_sim::planner::mcts::macro_net::{hierarchical_policy_net_dot, num_plan_edges};
use faf_sim::planner::mcts::train::{
    load_policy, save_policy, train_policy, train_policy_from, TrainConfig,
};
use faf_sim::planner::plan_graph::PlanNode;
use faf_sim::{
    run_build_order_simulation, Goal, GraphState, NodeId, PlanEdgeKind, Planner, SimulationConfig,
    Strategy, UnitKind as SimUnitKind, Units as SimUnits,
};
use faf_units::DataIndex;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;

use petgraph_svg::{graph_to_svg, EdgeLabel, LegendItem, NodeLabel, RenderOptions};

mod cmdline;
mod target;

use cmdline::{Cli, Command as CliCommand, DrawNetArgs, FactionTarget, TrainArgs};
use target::{Faction, ResearchTarget, UnitKind};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = Cli::parse();
    let index = load_index();
    let units = SimUnits::new(index.clone());

    match cli.command {
        CliCommand::Plan(args) => {
            run_plan(&units, &index, args.output);
        }
        CliCommand::Train(args) => {
            let (faction, unit) = resolve_faction_target(&args.target);
            let target = resolve_target(faction, unit);
            run_train(&units, target, args).await;
        }
        CliCommand::Simulate(args) => {
            let (faction, unit) = resolve_faction_target(&args.target);
            let target = resolve_target(faction, unit);
            run_simulate(&units, target, args.strategy, args.output).await;
        }
        CliCommand::DrawNet(args) => {
            let (faction, unit) = resolve_faction_target(&args.target);
            let target = resolve_target(faction, unit);
            run_draw_net(&units, target, args);
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

async fn run_train(units: &SimUnits, target: ResearchTarget, args: TrainArgs) {
    let goal = target.to_goal(units);

    let use_tui = !args.quiet && !args.no_tui && std::io::stdout().is_terminal();

    if !use_tui {
        println!(
            "Training MLP for {} {}",
            target.faction.display_name(),
            target.unit.display_name()
        );
    }

    let config = TrainConfig {
        episodes: args.episodes,
        max_steps: args.max_steps,
        target_time: args.target_time,
        epsilon: args.epsilon,
        epsilon_final: args.epsilon_final,
        epsilon_decay_episodes: if args.no_epsilon_decay {
            0
        } else {
            args.epsilon_decay_episodes.unwrap_or(args.episodes)
        },
        patience: args.patience,
        grad_clip: args.grad_clip,
        verbose: !args.quiet && !use_tui,
        ..Default::default()
    };

    let path = model_path(&target);
    let model_file = path.with_extension("mpk");
    let num_edges = num_plan_edges(units, &goal).expect("goal must have a plan graph");

    if args.fresh && model_file.exists() {
        println!(
            "Removing existing model checkpoint: {}",
            model_file.display()
        );
        std::fs::remove_file(&model_file).expect("failed to remove existing model file");
    }

    // Shared stop flag used by the trainer, the TUI, and the outer signal
    // handler. Setting it requests a graceful stop at the next episode boundary.
    let stop_flag = Arc::new(AtomicBool::new(false));
    // Hint flag for the TUI to display when the user presses Ctrl+C. Training
    // does not stop on Ctrl+C in the TUI; Ctrl+D is the normal stop key.
    let ctrl_c_hint = Arc::new(AtomicBool::new(false));
    let resume = args.resume;

    let (model, best_model, stats) = if use_tui {
        // The training closure runs on its own thread, so it needs owned data.
        let units = units.clone();
        let goal = goal;
        let path = path.clone();
        let model_file = model_file.clone();
        let flag = Arc::clone(&stop_flag);
        let hint = Arc::clone(&ctrl_c_hint);
        run_training_with_shutdown(
            move || {
                faf_sim_tui::TrainingDashboard::run(
                    Some(Arc::clone(&flag)),
                    Some(Arc::clone(&hint)),
                    move |observer| {
                        if resume && model_file.exists() {
                            let model = load_policy(&path, num_edges).expect("load existing model");
                            train_policy_from(
                                model,
                                &units,
                                &goal,
                                config,
                                observer,
                                Some(Arc::clone(&flag)),
                            )
                        } else {
                            train_policy(&units, &goal, config, observer, Some(Arc::clone(&flag)))
                        }
                    },
                )
            },
            stop_flag,
            Some(Arc::clone(&ctrl_c_hint)),
            use_tui,
        )
        .await
    } else if resume && model_file.exists() {
        println!("Resuming training from {}", model_file.display());
        let units = units.clone();
        let goal = goal;
        let path = path.clone();
        let flag = Arc::clone(&stop_flag);
        run_training_with_shutdown(
            move || {
                let model = load_policy(&path, num_edges).expect("load existing model");
                train_policy_from(model, &units, &goal, config, (), Some(Arc::clone(&flag)))
            },
            stop_flag,
            Some(Arc::clone(&ctrl_c_hint)),
            use_tui,
        )
        .await
    } else {
        let units = units.clone();
        let goal = goal;
        let flag = Arc::clone(&stop_flag);
        run_training_with_shutdown(
            move || train_policy(&units, &goal, config, (), Some(Arc::clone(&flag))),
            stop_flag,
            Some(Arc::clone(&ctrl_c_hint)),
            use_tui,
        )
        .await
    };

    println!(
        "Training complete: {}/{} episodes reached the goal",
        stats.goal_reaches,
        stats.episode_lengths.len()
    );
    if let Some(&best) = stats
        .completion_times
        .iter()
        .min_by(|a, b| a.partial_cmp(b).unwrap())
    {
        println!("Best completion time: {}", format_time(best));
    }

    // Save the best-seen model if there is one, otherwise the final model.
    let model_to_save = best_model.as_ref().unwrap_or(&model);
    save_policy(model_to_save, &path).expect("save trained model");
    if best_model.is_some() {
        println!("Saved best-seen model to {}", path.display());
    } else {
        println!("Model saved to {}", path.display());
    }
}

/// Run a blocking training closure to completion.
///
/// Outside the TUI, `Ctrl+C`/`SIGINT` requests a graceful stop and the best-seen
/// model is saved. Inside the TUI, `Ctrl+C` only shows a warning; the user must
/// press `Ctrl+D` to stop training gracefully.
async fn run_training_with_shutdown<F, T>(
    training: F,
    stop_flag: Arc<AtomicBool>,
    ctrl_c_hint: Option<Arc<AtomicBool>>,
    use_tui: bool,
) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let task = tokio::task::spawn_blocking(training);
    tokio::pin!(task);

    loop {
        tokio::select! {
            res = &mut task => return res.expect("training task panicked"),
            _ = tokio::signal::ctrl_c() => {
                if use_tui {
                    // The TUI is in raw mode, so an actual SIGINT is unlikely to
                    // come from the keyboard, but if it does, mirror the on-screen
                    // Ctrl+C warning and keep running.
                    if let Some(hint) = &ctrl_c_hint {
                        hint.store(true, Ordering::Relaxed);
                    }
                } else {
                    eprintln!("Ctrl+C received; stopping training gracefully.");
                    stop_flag.store(true, Ordering::Relaxed);
                    return task.await.expect("training task panicked");
                }
            }
        }
    }
}

fn run_draw_net(units: &SimUnits, target: ResearchTarget, args: DrawNetArgs) {
    let goal = target.to_goal(units);
    let num_edges = num_plan_edges(units, &goal).expect("target must have a plan graph");
    let dot = hierarchical_policy_net_dot(num_edges);

    let dot_path = match args.output {
        Some(path) => path,
        None => {
            let file_name = format!(
                "faf-sim-net-{}-{}.dot",
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

    std::fs::write(&dot_path, dot).expect("write DOT file");
    println!("Wrote network architecture DOT to {}", dot_path.display());

    // Render to SVG if Graphviz `dot` is available.
    let svg_path = dot_path.with_extension("svg");
    match std::process::Command::new("dot")
        .args([
            "-Tsvg",
            dot_path.to_str().unwrap(),
            "-o",
            svg_path.to_str().unwrap(),
        ])
        .status()
    {
        Ok(status) if status.success() => {
            println!("Rendered SVG to {}", svg_path.display());
            if let Ok(absolute) = std::fs::canonicalize(&svg_path) {
                println!("  file://{}", absolute.display());
            }
        }
        _ => {
            eprintln!("Graphviz `dot` not available; SVG rendering skipped.");
            eprintln!(
                "You can render manually with: dot -Tsvg {} -o {}",
                dot_path.display(),
                svg_path.display()
            );
        }
    }
}

async fn run_simulate(
    units: &SimUnits,
    target: ResearchTarget,
    strategy: Strategy,
    output: Option<std::path::PathBuf>,
) {
    use faf_sim::PlannerConfig;

    let goal = target.to_goal(units);

    println!("Strategy: {}", strategy);
    println!("Simulate target: {}", target.display_name());

    let path = model_path(&target);
    let model_file = path.with_extension("mpk");

    let planner = if model_file.exists() {
        println!("Loading trained model from {}", model_file.display());
        let num_edges = num_plan_edges(units, &goal).expect("goal must have a plan graph");
        let model = load_policy(&path, num_edges).expect("load trained model");
        Planner::with_value_net(strategy, PlannerConfig::default(), model)
    } else {
        println!("No trained model found; using random initialization");
        Planner::reactive(strategy)
    };

    let config = SimulationConfig {
        planner,
        sim_dt: 1.0,
        max_sim_time: 8.0 * 60.0 * 60.0,
    };
    let result = match run_build_order_simulation(units.clone(), goal, config).await {
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
    println!("\nFinal economy:");
    println!(
        "  Mass income:  {:.1} / s",
        result.final_state.economy.net_mass_income
    );
    println!(
        "  Energy income: {:.1} / s",
        result.final_state.economy.net_energy_income
    );
    println!(
        "  Mass storage:  {:.0} / {:.0}",
        result.final_state.economy.mass_storage, result.final_state.economy.mass_storage_cap
    );
    println!(
        "  Energy storage: {:.0} / {:.0}",
        result.final_state.economy.energy_storage, result.final_state.economy.energy_storage_cap
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

fn run_plan(units: &SimUnits, _index: &DataIndex, output: Option<std::path::PathBuf>) {
    // Render the universal graph with a placeholder abstract target so the SVG
    // shows the T3-engineer-only goal edge.
    let placeholder_goal = Goal {
        tech_level: faf_sim::units::TechLevel::T4,
        mass_cost: 0.0,
        energy_cost: 0.0,
        build_time: 0.0,
    };
    let plan = units.plan_graph(placeholder_goal);
    let visual_graph = build_visual_graph(units, plan.graph());

    let path = match output {
        Some(path) => path,
        None => std::env::temp_dir().join("faf-sim-plan-universal.svg"),
    };

    let options = RenderOptions {
        background_color: Some("#f8fafc".to_string()),
        orientation: petgraph_svg::Orientation::TopToBottom,
        node_width: 150.0,
        node_height: 48.0,
        node_gap_x: 24.0,
        level_gap_y: 72.0,
        font_size: 11.0,
        stroke_width: 1.0,
        margin_x: 60.0,
        margin_y: 60.0,
        legend: vec![
            LegendItem::solid("build", "#555555"),
            LegendItem::dashed("upgrade", "#0066cc", "5,5"),
            LegendItem::solid("ACU", "#e2e8f0"),
            LegendItem::solid("T1", "#dcfce7"),
            LegendItem::solid("T2", "#fef9c3"),
            LegendItem::solid("T3", "#fee2e2"),
            LegendItem::solid("T4 / unique", "#f3e8ff"),
            LegendItem::solid("Target", "#f3e8ff"),
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
                color: "#94a3b8".to_string(),
                dash: None,
            },
            PlanEdgeKind::Upgrade => Self {
                label: String::new(),
                color: "#3b82f6".to_string(),
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

/// A rendered node with a tech-tier fill colour.
#[derive(Debug, Clone)]
struct VisualNode {
    label: String,
    color: Option<String>,
}

impl NodeLabel for VisualNode {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn color(&self) -> Option<String> {
        self.color.clone()
    }
}

/// Build a labelled petgraph from the ACU-rooted plan graph for rendering.
///
/// The returned graph preserves the structure of the input graph but attaches
/// human-readable labels, edge styles, and tier-based fill colours.
fn build_visual_graph(
    units: &SimUnits,
    plan_graph: &petgraph::graph::DiGraph<PlanNode, PlanEdgeKind>,
) -> petgraph::graph::DiGraph<VisualNode, VisualEdge> {
    let mut graph = petgraph::graph::DiGraph::<VisualNode, VisualEdge>::new();
    let mut indices: Vec<NodeIndex> = Vec::with_capacity(plan_graph.node_count());

    for node in plan_graph.node_indices() {
        let node_ref = &plan_graph[node];
        indices.push(graph.add_node(VisualNode {
            label: node_label(units, node_ref),
            color: Some(tier_color(node_ref)),
        }));
    }

    for edge in plan_graph.edge_references() {
        let from = indices[edge.source().index()];
        let to = indices[edge.target().index()];
        graph.add_edge(from, to, VisualEdge::for_kind(*edge.weight()));
    }

    graph
}

fn node_label(units: &SimUnits, node: &PlanNode) -> String {
    match node {
        PlanNode::Goal(_) => "Target\n(T3 engineer only)".to_string(),
        PlanNode::Unit(kind) => unit_node_label(units, kind),
    }
}

fn unit_node_label(units: &SimUnits, kind: &SimUnitKind) -> String {
    use faf_sim::UnitKind;
    match kind {
        UnitKind::Commander => "ACU".to_string(),
        UnitKind::Engineer(tl) => format!("T{} Eng", tier_number(*tl)),
        UnitKind::Factory(tl) => format!("T{} Factory", tier_number(*tl)),
        UnitKind::Mex(tl) => format!("T{} Mex", tier_number(*tl)),
        UnitKind::Pgen(tl) => format!("T{} PGen", tier_number(*tl)),
        UnitKind::CapT2Mex => "T2 Mex Capped".to_string(),
        UnitKind::CapT3Mex => "T3 Mex Capped".to_string(),
        UnitKind::EnergyStorage => "Energy Storage".to_string(),
        UnitKind::Unique(_) => units.display_name(kind).to_string(),
    }
}

fn tier_number(tl: faf_sim::TechLevel) -> u8 {
    match tl {
        faf_sim::TechLevel::T1 => 1,
        faf_sim::TechLevel::T2 => 2,
        faf_sim::TechLevel::T3 => 3,
        faf_sim::TechLevel::T4 => 4,
    }
}

/// Fill colour for a node based on its tech tier.
fn tier_color(node: &PlanNode) -> String {
    match node {
        PlanNode::Goal(_) => "#f3e8ff".to_string(), // purple-100
        PlanNode::Unit(kind) => unit_tier_color(kind),
    }
}

fn unit_tier_color(kind: &SimUnitKind) -> String {
    use faf_sim::UnitKind;
    match kind {
        UnitKind::Commander => "#e2e8f0".to_string(), // slate-200
        UnitKind::Engineer(tl) | UnitKind::Factory(tl) | UnitKind::Mex(tl) | UnitKind::Pgen(tl) => {
            match tl {
                faf_sim::TechLevel::T1 => "#dcfce7".to_string(), // green-100
                faf_sim::TechLevel::T2 => "#fef9c3".to_string(), // yellow-100
                faf_sim::TechLevel::T3 => "#fee2e2".to_string(), // red-100
                faf_sim::TechLevel::T4 => "#f3e8ff".to_string(), // purple-100
            }
        }
        UnitKind::CapT2Mex => "#fef9c3".to_string(),
        UnitKind::CapT3Mex => "#fee2e2".to_string(),
        UnitKind::EnergyStorage => "#e0f2fe".to_string(), // sky-100
        UnitKind::Unique(_) => "#f3e8ff".to_string(),     // purple-100
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
        let name = unit_node_label(units, &slot.unit_id);

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
                label.push_str(&format!("\n(was {})", unit_node_label(units, original)));
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
