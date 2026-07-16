//! Blueprint dependency graph data for the scheduler page.
//!
//! Serializes the economic/builder subgraph as JSON. The wire format matches
//! `faf_dioxus_ui::GraphData`:
//!
//! ```json
//! {
//!   "nodes": [{ "label": "ACU", "color": "#fbbf24" }],
//!   "edges": [[0, 1, { "label": "built by", "color": "#38bdf8", "dashed": false }]]
//! }
//! ```
//!
//! Edges point from prerequisite/builder to the dependent unit so that the
//! ACU (which builds everything and requires nothing) is the root source.

use faf_sim::units::{role_of, BlueprintLibrary, TechLevel, UnitKind, UnitRole};
use faf_units::DataIndex;
use serde::Serialize;

const ROLES_TO_SHOW: &[UnitRole] = &[
    UnitRole::Commander,
    UnitRole::Engineer,
    UnitRole::Factory,
    UnitRole::MassExtractor,
    UnitRole::PowerGenerator,
    UnitRole::EnergyStorage,
    UnitRole::CappedMassExtractor,
];

#[derive(Debug, Clone, Serialize)]
struct GraphNodeJson {
    label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    color: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct GraphEdgeJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    color: Option<String>,
    dashed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlueprintGraphJson {
    nodes: Vec<GraphNodeJson>,
    edges: Vec<(usize, usize, GraphEdgeJson)>,
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
        UnitKind::Unique(id) => id.0.clone(),
    }
}

fn should_show(kind: &UnitKind) -> bool {
    if !ROLES_TO_SHOW.contains(&role_of(kind)) {
        return false;
    }
    // T4 economic/builder units don't exist in the real game data.
    match kind {
        UnitKind::Factory(TechLevel::T4)
        | UnitKind::Pgen(TechLevel::T4)
        | UnitKind::Mex(TechLevel::T4)
        | UnitKind::Engineer(TechLevel::T4) => false,
        _ => true,
    }
}

/// Build the economic/builder blueprint graph as a JSON-serializable payload.
pub fn economic_graph_json(index: &DataIndex) -> BlueprintGraphJson {
    let library = BlueprintLibrary::new(index.clone());
    let graph = library.build_graph();

    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut indices = std::collections::HashMap::new();

    for node in &graph.nodes {
        if !should_show(&node.kind) {
            continue;
        }
        indices.insert(node.kind.clone(), nodes.len());
        nodes.push(GraphNodeJson {
            label: node_label(&node.kind),
            color: Some(role_color(node.role).to_string()),
        });
    }

    for edge in &graph.build_edges {
        if !should_show(&edge.target) {
            continue;
        }
        let target = *indices.get(&edge.target).expect("target node inserted");

        // Prerequisite flows into the dependent unit.
        if let Some(prereq) = &edge.prereq {
            if should_show(prereq) {
                if let Some(&prereq_idx) = indices.get(prereq) {
                    edges.push((
                        prereq_idx,
                        target,
                        GraphEdgeJson {
                            label: None,
                            color: Some("#94a3b8".to_string()),
                            dashed: true,
                        },
                    ));
                }
            }
        }

        // Builder flows into the unit it builds.
        for builder in &edge.builders {
            if !should_show(builder) {
                continue;
            }
            if let Some(&builder_idx) = indices.get(builder) {
                edges.push((
                    builder_idx,
                    target,
                    GraphEdgeJson {
                        label: None,
                        color: Some("#38bdf8".to_string()),
                        dashed: false,
                    },
                ));
            }
        }
    }

    for edge in &graph.upgrade_edges {
        if !should_show(&edge.from) || !should_show(&edge.to) {
            continue;
        }
        let from = *indices.get(&edge.from).expect("from node inserted");
        let to = *indices.get(&edge.to).expect("to node inserted");
        edges.push((
            from,
            to,
            GraphEdgeJson {
                label: None,
                color: Some("#fbbf24".to_string()),
                dashed: true,
            },
        ));
    }

    BlueprintGraphJson { nodes, edges }
}
