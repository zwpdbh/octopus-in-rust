use std::collections::HashMap;

use dioxus::prelude::*;
use faf_dioxus_ui::components::{GraphEdgeData, GraphNodeData, GraphView};
use faf_sim::units::{role_of, tech_level_of, BlueprintEdge, TechLevel, UnitKind, UnitRole};
use faf_sim_shared::plan::UnitSummary;
use petgraph::visit::EdgeRef;

use crate::types::BlueprintGraphData;

/// Render an interactive blueprint dependency graph with AntV G6.
///
/// Accepts the raw [`BlueprintGraph`] from the server plus a map of unit
/// summaries keyed by node id. All rendering configuration (colors, layers,
/// edge styles, filtering, deduplication) lives in this component.
#[component]
pub fn BlueprintGraph(
    graph: faf_sim::units::BlueprintGraph,
    summaries: HashMap<String, UnitSummary>,
    on_node_click: EventHandler<UnitSummary>,
) -> Element {
    let data = use_memo(move || to_graph_data(&graph, &summaries));
    let data_for_lookup = data();

    rsx! {
        GraphView {
            id: "blueprint-graph".to_string(),
            data: data(),
            on_node_click: move |id: String| {
                if let Some(node) = data_for_lookup.node_by_id(&id) {
                    if let Some(summary) = node.data.clone() {
                        on_node_click.call(summary);
                    }
                }
            },
        }
    }
}

fn to_graph_data(
    graph: &faf_sim::units::BlueprintGraph,
    summaries: &HashMap<String, UnitSummary>,
) -> BlueprintGraphData {
    let mut nodes = Vec::new();
    let mut ids = HashMap::new();

    for node in graph.graph.node_weights() {
        if !should_show(&node.kind) {
            continue;
        }
        let id = node_id(&node.kind);
        let summary = summaries.get(&id).cloned();
        ids.insert(node.kind.clone(), id.clone());
        nodes.push(GraphNodeData {
            id,
            label: node_label(&node.kind),
            color: Some(role_color(node.role).to_string()),
            layer: Some(node_layer(&node.kind)),
            data: summary,
        });
    }

    // Deduplicate edges between the same pair of nodes. When a build rule and
    // an upgrade rule describe the same dependency (e.g. T1 factory -> T2
    // factory), keep the single most informative edge.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum EdgePriority {
        Prereq,
        Builder,
        Upgrade,
    }

    let mut edge_map: HashMap<(String, String), (EdgePriority, GraphEdgeData)> = HashMap::new();

    let mut add_edge =
        |source: String, target: String, priority: EdgePriority, edge: GraphEdgeData| {
            let key = (source, target);
            if let Some((existing_priority, _)) = edge_map.get(&key) {
                if priority <= *existing_priority {
                    return;
                }
            }
            edge_map.insert(key, (priority, edge));
        };

    for edge in graph.graph.edge_references() {
        let source_kind = &graph.graph[edge.source()].kind;
        let target_kind = &graph.graph[edge.target()].kind;

        if !should_show(source_kind) || !should_show(target_kind) {
            continue;
        }

        let source_id = ids.get(source_kind).expect("source node inserted").clone();
        let target_id = ids.get(target_kind).expect("target node inserted").clone();
        let target_layer = node_layer(target_kind);

        match edge.weight() {
            BlueprintEdge::BuiltBy { ref prereq } => {
                if node_layer(source_kind) > target_layer {
                    // Skip cross-tier builder edges (e.g. T3 engineer -> T2 factory)
                    // so that the dagre layout can keep clean tech-level columns.
                    continue;
                }

                add_edge(
                    source_id.clone(),
                    target_id.clone(),
                    EdgePriority::Builder,
                    GraphEdgeData {
                        source: source_id.clone(),
                        target: target_id.clone(),
                        color: Some("#38bdf8".to_string()),
                        dashed: false,
                        data: None,
                    },
                );

                if let Some(ref prereq) = prereq {
                    if should_show(prereq) && node_layer(prereq) <= target_layer {
                        if let Some(prereq_id) = ids.get(prereq) {
                            add_edge(
                                prereq_id.clone(),
                                target_id.clone(),
                                EdgePriority::Prereq,
                                GraphEdgeData {
                                    source: prereq_id.clone(),
                                    target: target_id.clone(),
                                    color: Some("#94a3b8".to_string()),
                                    dashed: true,
                                    data: None,
                                },
                            );
                        }
                    }
                }
            }
            BlueprintEdge::UpgradesInto { .. } => {
                if node_layer(source_kind) > node_layer(target_kind) {
                    continue;
                }
                add_edge(
                    source_id.clone(),
                    target_id.clone(),
                    EdgePriority::Upgrade,
                    GraphEdgeData {
                        source: source_id,
                        target: target_id,
                        color: Some("#fbbf24".to_string()),
                        dashed: true,
                        data: None,
                    },
                );
            }
        }
    }

    let edges = edge_map.into_values().map(|(_, e)| e).collect();

    BlueprintGraphData { nodes, edges }
}

fn role_color(role: UnitRole) -> &'static str {
    match role {
        UnitRole::Commander => "#fbbf24",
        UnitRole::Engineer => "#60a5fa",
        UnitRole::Factory => "#a78bfa",
        UnitRole::MassExtractor => "#34d399",
        UnitRole::PowerGenerator => "#f87171",
        UnitRole::EnergyStorage => "#f472b6",
        UnitRole::CappedMassExtractor => "#2dd4bf",
        UnitRole::Experimental => "#f97316",
        UnitRole::Other => "#9ca3af",
    }
}

fn node_label(kind: &UnitKind) -> String {
    match kind {
        UnitKind::Commander => "ACU".to_string(),
        UnitKind::Engineer(t) => format!("Eng {t:?}"),
        UnitKind::Factory(t) => format!("Factory {t:?}"),
        UnitKind::Mex(t) => format!("Mex {t:?}"),
        UnitKind::Pgen(t) => format!("Pgen {t:?}"),
        UnitKind::CapT2Mex => "Cap T2 Mex".to_string(),
        UnitKind::CapT3Mex => "Cap T3 Mex".to_string(),
        UnitKind::EnergyStorage => "Energy Storage".to_string(),
        UnitKind::Experimental => "Experimental".to_string(),
        UnitKind::Unique(id) => id.0.clone(),
    }
}

/// Dagre layer used to group nodes into tech-level columns.
///
/// Columns: ACU (0), T1 (1), T2 (2), T3 (3), T4 (4).
fn node_layer(kind: &UnitKind) -> i32 {
    match kind {
        UnitKind::Commander => 0,
        UnitKind::EnergyStorage => 1,
        UnitKind::CapT2Mex => 2,
        UnitKind::CapT3Mex => 3,
        UnitKind::Experimental => 4,
        kind => tech_level_of(kind)
            .map(|tech| match tech {
                TechLevel::T1 => 1,
                TechLevel::T2 => 2,
                TechLevel::T3 => 3,
                TechLevel::T4 => 4,
            })
            .unwrap_or(0),
    }
}

fn node_id(kind: &UnitKind) -> String {
    match kind {
        UnitKind::Unique(id) => id.0.clone(),
        _ => format!("{kind:?}"),
    }
}

const ROLES_TO_SHOW: &[UnitRole] = &[
    UnitRole::Commander,
    UnitRole::Engineer,
    UnitRole::Factory,
    UnitRole::MassExtractor,
    UnitRole::PowerGenerator,
    UnitRole::EnergyStorage,
    UnitRole::CappedMassExtractor,
    UnitRole::Experimental,
];

fn should_show(kind: &UnitKind) -> bool {
    if !ROLES_TO_SHOW.contains(&role_of(kind)) {
        return false;
    }
    !matches!(
        kind,
        UnitKind::Factory(TechLevel::T4)
            | UnitKind::Pgen(TechLevel::T4)
            | UnitKind::Mex(TechLevel::T4)
            | UnitKind::Engineer(TechLevel::T4)
    )
}
