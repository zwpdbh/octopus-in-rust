use std::collections::HashMap;

use dioxus::prelude::*;
use faf_dioxus_ui::components::{GraphEdgeData, GraphNodeData, GraphView};
use faf_sim_shared::plan::UnitSummary;

use crate::types::{BlueprintGraphData, ConcreteEdgeKind, ConcreteGraphEdge, ConcreteGraphNode};

/// Render a faction subgraph of the concrete blueprint relationship graph.
///
/// Nodes carry the real unit portrait (`icon`) and the focused unit (if any)
/// is highlighted. This component only maps data to the generic `GraphView`;
/// the graph itself is built server-side from the unit index.
#[component]
pub fn BlueprintGraph(
    nodes: Vec<ConcreteGraphNode>,
    edges: Vec<ConcreteGraphEdge>,
    summaries: HashMap<String, UnitSummary>,
    /// Only nodes of this faction are shown (their subgraph).
    faction: String,
    /// Blueprint id of the node to highlight, if any.
    highlight: Option<String>,
    on_node_click: EventHandler<UnitSummary>,
) -> Element {
    // Track the raw props as reactive dependencies so the faction subgraph is
    // recomputed when the caller changes faction/highlight.
    let data =
        use_memo(use_reactive!(|nodes,
                                edges,
                                summaries,
                                faction,
                                highlight| {
            to_graph_data(&nodes, &edges, &summaries, &faction, &highlight)
        }));
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
    nodes: &[ConcreteGraphNode],
    edges: &[ConcreteGraphEdge],
    summaries: &HashMap<String, UnitSummary>,
    faction: &str,
    highlight: &Option<String>,
) -> BlueprintGraphData {
    let in_faction: std::collections::HashSet<&str> = nodes
        .iter()
        .filter(|n| n.faction == faction)
        .map(|n| n.id.as_str())
        .collect();

    let nodes_out: Vec<GraphNodeData<UnitSummary>> = nodes
        .iter()
        .filter(|n| in_faction.contains(n.id.as_str()))
        .map(|node| GraphNodeData {
            id: node.id.clone(),
            label: node.display_name.clone(),
            color: Some(node.role.color().to_string()),
            layer: Some(node.layer),
            icon: Some(format!("/api/portraits/{}.png", node.id)),
            highlight: if highlight.as_deref() == Some(node.id.as_str()) {
                Some(true)
            } else {
                None
            },
            data: summaries.get(&node.id).cloned(),
        })
        .collect();

    // For the same (source, target) prefer the upgrade edge over the built-by
    // edge; also drop edges that leave the faction subgraph.
    let mut best: HashMap<(&str, &str), &ConcreteGraphEdge> = HashMap::new();
    for edge in edges {
        if !in_faction.contains(edge.source.as_str()) || !in_faction.contains(edge.target.as_str())
        {
            continue;
        }
        let key = (edge.source.as_str(), edge.target.as_str());
        best.entry(key)
            .and_modify(|existing| {
                if existing.kind == ConcreteEdgeKind::BuiltBy
                    && edge.kind == ConcreteEdgeKind::UpgradesInto
                {
                    *existing = edge;
                }
            })
            .or_insert(edge);
    }

    let edges_out: Vec<GraphEdgeData> = best
        .into_values()
        .map(|edge| match edge.kind {
            ConcreteEdgeKind::BuiltBy => GraphEdgeData {
                source: edge.source.clone(),
                target: edge.target.clone(),
                color: Some("#38bdf8".to_string()),
                dashed: false,
                data: None,
            },
            ConcreteEdgeKind::UpgradesInto => GraphEdgeData {
                source: edge.source.clone(),
                target: edge.target.clone(),
                color: Some("#fbbf24".to_string()),
                dashed: true,
                data: None,
            },
        })
        .collect();

    BlueprintGraphData {
        nodes: nodes_out,
        edges: edges_out,
    }
}
