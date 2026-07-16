//! Blueprint dependency graph data for the scheduler page.
//!
//! Serializes the economic/builder subgraph as JSON. The generic wire format
//! matches `faf_dioxus_ui::GraphData`; the G6-specific format adds a stable
//! blueprint id and unit summary to every node so the frontend can show unit
//! details on click.
//!
//! Edges point from prerequisite/builder to the dependent unit so that the
//! ACU (which builds everything and requires nothing) is the root source.

use faf_sim::units::{role_of, tech_level_of, BlueprintLibrary, TechLevel, UnitKind, UnitRole};
use faf_units::{DataIndex, Unit};
use serde::Serialize;

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

#[derive(Debug, Clone, Serialize)]
struct GraphNodeJson {
    id: String,
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

#[derive(Debug, Clone, Serialize)]
pub struct G6NodeJson {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub layer: i32,
    pub summary: crate::UnitSummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct G6EdgeJson {
    pub source: String,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub dashed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct G6GraphJson {
    pub nodes: Vec<G6NodeJson>,
    pub edges: Vec<G6EdgeJson>,
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

/// Stable unique identifier for a graph node.
///
/// `BlueprintLibrary::blueprint_id` collapses `CapT2Mex`/`CapT3Mex` onto the
/// base extractor id, so we derive the id from the abstract kind instead.
fn node_id(kind: &UnitKind) -> String {
    match kind {
        UnitKind::Unique(id) => id.0.clone(),
        _ => format!("{kind:?}"),
    }
}

fn should_show(kind: &UnitKind) -> bool {
    if !ROLES_TO_SHOW.contains(&role_of(kind)) {
        return false;
    }
    // T4 economic/builder units don't exist in the real game data.
    !matches!(
        kind,
        UnitKind::Factory(TechLevel::T4)
            | UnitKind::Pgen(TechLevel::T4)
            | UnitKind::Mex(TechLevel::T4)
            | UnitKind::Engineer(TechLevel::T4)
    )
}

fn unit_summary(unit: &Unit) -> crate::UnitSummary {
    crate::UnitSummary {
        id: unit.id.clone(),
        display_name: unit.name().unwrap_or(&unit.id).to_string(),
        faction: unit.faction().unwrap_or("Unknown").to_string(),
        tech: unit.tech_level().unwrap_or("TECH1").to_string(),
        category: crate::browser_category(unit).label().to_string(),
        strategic_icon_name: unit.strategic_icon_name.clone(),
        kind: crate::unit_kind(unit).to_string(),
        build_rate: unit.economy.as_ref().and_then(|e| e.build_rate),
        build_cost_mass: unit.economy.as_ref().and_then(|e| e.build_cost_mass),
        build_cost_energy: unit.economy.as_ref().and_then(|e| e.build_cost_energy),
        build_time: unit.economy.as_ref().and_then(|e| e.build_time),
        production_per_second_mass: unit
            .economy
            .as_ref()
            .and_then(|e| e.production_per_second_mass),
        production_per_second_energy: unit
            .economy
            .as_ref()
            .and_then(|e| e.production_per_second_energy),
        maintenance_consumption_per_second_energy: unit
            .economy
            .as_ref()
            .and_then(|e| e.maintenance_consumption_per_second_energy),
        mass_storage: unit.economy.as_ref().and_then(|e| e.storage_mass),
        energy_storage: unit.economy.as_ref().and_then(|e| e.storage_energy),
    }
}

/// Build the economic/builder blueprint graph as the generic JSON payload.
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
        let id = node_id(&node.kind);
        indices.insert(node.kind.clone(), nodes.len());
        nodes.push(GraphNodeJson {
            id,
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

/// Build the economic/builder blueprint graph as a G6-shaped payload.
pub fn economic_graph_g6_json(index: &DataIndex) -> G6GraphJson {
    let library = BlueprintLibrary::new(index.clone());
    let graph = library.build_graph();

    let mut nodes = Vec::new();
    let mut indices = std::collections::HashMap::new();
    let mut ids = std::collections::HashMap::new();

    for node in &graph.nodes {
        if !should_show(&node.kind) {
            continue;
        }
        let id = node_id(&node.kind);
        let blueprint_id = library
            .blueprint_id(&node.kind)
            .unwrap_or_else(|| node_label(&node.kind));
        let summary = index
            .find_unit(&blueprint_id)
            .map(unit_summary)
            .unwrap_or_else(|| crate::UnitSummary {
                id: blueprint_id,
                display_name: node.display_name.clone(),
                faction: "Unknown".to_string(),
                tech: "TECH1".to_string(),
                category: crate::BrowserCategory::Land.label().to_string(),
                strategic_icon_name: None,
                kind: "Unknown".to_string(),
                build_rate: None,
                build_cost_mass: None,
                build_cost_energy: None,
                build_time: None,
                production_per_second_mass: None,
                production_per_second_energy: None,
                maintenance_consumption_per_second_energy: None,
                mass_storage: None,
                energy_storage: None,
            });
        indices.insert(node.kind.clone(), nodes.len());
        ids.insert(node.kind.clone(), id.clone());
        nodes.push(G6NodeJson {
            id,
            label: node_label(&node.kind),
            color: Some(role_color(node.role).to_string()),
            layer: node_layer(&node.kind),
            summary,
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

    let mut edge_map: std::collections::HashMap<(String, String), (EdgePriority, G6EdgeJson)> =
        std::collections::HashMap::new();

    let mut add_edge =
        |source: String, target: String, priority: EdgePriority, edge: G6EdgeJson| {
            let key = (source, target);
            if let Some((existing_priority, _)) = edge_map.get(&key) {
                if priority <= *existing_priority {
                    return;
                }
            }
            edge_map.insert(key, (priority, edge));
        };

    for edge in &graph.build_edges {
        if !should_show(&edge.target) {
            continue;
        }
        let target_id = ids.get(&edge.target).expect("target node inserted").clone();
        let target_layer = node_layer(&edge.target);

        if let Some(prereq) = &edge.prereq {
            if should_show(prereq) && node_layer(prereq) <= target_layer {
                if let Some(prereq_id) = ids.get(prereq) {
                    add_edge(
                        prereq_id.clone(),
                        target_id.clone(),
                        EdgePriority::Prereq,
                        G6EdgeJson {
                            source: prereq_id.clone(),
                            target: target_id.clone(),
                            color: Some("#94a3b8".to_string()),
                            dashed: true,
                        },
                    );
                }
            }
        }

        for builder in &edge.builders {
            if !should_show(builder) {
                continue;
            }
            if node_layer(builder) > target_layer {
                // Skip cross-tier builder edges (e.g. T3 engineer -> T2 factory)
                // so that the dagre layout can keep clean tech-level columns.
                continue;
            }
            if let Some(builder_id) = ids.get(builder) {
                add_edge(
                    builder_id.clone(),
                    target_id.clone(),
                    EdgePriority::Builder,
                    G6EdgeJson {
                        source: builder_id.clone(),
                        target: target_id.clone(),
                        color: Some("#38bdf8".to_string()),
                        dashed: false,
                    },
                );
            }
        }
    }

    for edge in &graph.upgrade_edges {
        if !should_show(&edge.from) || !should_show(&edge.to) {
            continue;
        }
        if node_layer(&edge.from) > node_layer(&edge.to) {
            continue;
        }
        let from_id = ids.get(&edge.from).expect("from node inserted").clone();
        let to_id = ids.get(&edge.to).expect("to node inserted").clone();
        add_edge(
            from_id.clone(),
            to_id.clone(),
            EdgePriority::Upgrade,
            G6EdgeJson {
                source: from_id,
                target: to_id,
                color: Some("#fbbf24".to_string()),
                dashed: true,
            },
        );
    }

    let edges: Vec<G6EdgeJson> = edge_map.into_values().map(|(_, e)| e).collect();

    G6GraphJson { nodes, edges }
}
