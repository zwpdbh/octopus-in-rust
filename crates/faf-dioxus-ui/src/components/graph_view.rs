//! Generic directed graph viewer component.
//!
//! Accepts a [`petgraph::DiGraph`] and renders it as an SVG with a hierarchical
//! layout. The graph is owned by the caller; the component only computes node
//! positions and emits SVG elements.

use dioxus::prelude::*;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// Options controlling the rendered SVG.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphOptions {
    pub node_width: f32,
    pub node_height: f32,
    pub node_gap_x: f32,
    pub level_gap_y: f32,
    pub margin_x: f32,
    pub margin_y: f32,
    pub font_size: f32,
    pub node_radius: f32,
    pub stroke_width: f32,
    pub min_width: u32,
    pub min_height: u32,
    pub background_color: Option<String>,
    pub orientation: GraphOrientation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphOrientation {
    TopToBottom,
    LeftToRight,
}

impl Default for GraphOptions {
    fn default() -> Self {
        Self {
            node_width: 140.0,
            node_height: 40.0,
            node_gap_x: 40.0,
            level_gap_y: 80.0,
            margin_x: 40.0,
            margin_y: 40.0,
            font_size: 12.0,
            node_radius: 6.0,
            stroke_width: 1.5,
            min_width: 600,
            min_height: 400,
            background_color: None,
            orientation: GraphOrientation::TopToBottom,
        }
    }
}

/// Types that can provide a label and optional color for a graph node.
pub trait GraphNodeLabel {
    fn label(&self) -> String;
    fn color(&self) -> Option<String> {
        None
    }
}

/// Types that can provide a label, color, and dash style for a graph edge.
pub trait GraphEdgeLabel {
    fn label(&self) -> Option<String> {
        None
    }
    fn color(&self) -> Option<String> {
        None
    }
    fn dashed(&self) -> bool {
        false
    }
}

impl GraphNodeLabel for String {
    fn label(&self) -> String {
        self.clone()
    }
}

impl GraphEdgeLabel for String {
    fn label(&self) -> Option<String> {
        Some(self.clone())
    }
}

/// Simple serializable node descriptor for fetching graphs over HTTP.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphNodeData {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub color: Option<String>,
}

impl GraphNodeLabel for GraphNodeData {
    fn label(&self) -> String {
        self.label.clone()
    }

    fn color(&self) -> Option<String> {
        self.color.clone()
    }
}

/// Simple serializable edge descriptor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphEdgeData {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub dashed: bool,
}

impl GraphEdgeLabel for GraphEdgeData {
    fn label(&self) -> Option<String> {
        self.label.clone()
    }

    fn color(&self) -> Option<String> {
        self.color.clone()
    }

    fn dashed(&self) -> bool {
        self.dashed
    }
}

/// Serializable graph payload used by the frontend to build a [`DiGraph`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphData {
    pub nodes: Vec<GraphNodeData>,
    pub edges: Vec<(usize, usize, GraphEdgeData)>,
}

impl From<&GraphData> for DiGraph<GraphNodeData, GraphEdgeData> {
    fn from(data: &GraphData) -> Self {
        let mut graph = DiGraph::new();
        for node in &data.nodes {
            graph.add_node(node.clone());
        }
        for (from, to, edge) in &data.edges {
            let from_idx = petgraph::graph::NodeIndex::new(*from);
            let to_idx = petgraph::graph::NodeIndex::new(*to);
            graph.add_edge(from_idx, to_idx, edge.clone());
        }
        graph
    }
}

impl GraphData {
    /// Look up a node by its `id`.
    pub fn node_by_id(&self, id: &str) -> Option<&GraphNodeData> {
        self.nodes.iter().find(|n| n.id == id)
    }
}

#[derive(Debug, Clone, Copy)]
struct NodePos {
    x: f32,
    y: f32,
}

/// A [`DiGraph`] wrapped for use as a Dioxus component prop.
///
/// `petgraph::Graph` does not implement `PartialEq`, which Dioxus props
/// require, so this newtype provides a value-based comparison of node weights
/// and edge triples (source index, target index, weight).
#[derive(Debug, Clone)]
pub struct GraphInput<N, E>(pub DiGraph<N, E>);

impl<N, E> From<DiGraph<N, E>> for GraphInput<N, E> {
    fn from(graph: DiGraph<N, E>) -> Self {
        Self(graph)
    }
}

impl<N: PartialEq, E: PartialEq> PartialEq for GraphInput<N, E> {
    fn eq(&self, other: &Self) -> bool {
        let a = &self.0;
        let b = &other.0;
        if a.node_count() != b.node_count() || a.edge_count() != b.edge_count() {
            return false;
        }
        if !a.node_weights().zip(b.node_weights()).all(|(x, y)| x == y) {
            return false;
        }
        fn sorted_edges<N, E>(g: &DiGraph<N, E>) -> Vec<(usize, usize, &E)> {
            let mut v: Vec<(usize, usize, &E)> = g
                .edge_references()
                .map(|e| (e.source().index(), e.target().index(), e.weight()))
                .collect();
            v.sort_by_key(|e| (e.0, e.1));
            v
        }
        let a_edges = sorted_edges(a);
        let b_edges = sorted_edges(b);
        a_edges
            .iter()
            .zip(b_edges.iter())
            .all(|(x, y)| x.0 == y.0 && x.1 == y.1 && x.2 == y.2)
    }
}

/// Render a directed graph as an SVG.
#[component]
pub fn GraphView<N, E>(graph: GraphInput<N, E>, options: GraphOptions) -> Element
where
    N: GraphNodeLabel + Clone + PartialEq + 'static,
    E: GraphEdgeLabel + Clone + PartialEq + 'static,
{
    let graph = graph.0;
    if graph.node_count() == 0 {
        return rsx! {
            div { class: "text-neutral-400 text-sm", "No graph data" }
        };
    }

    let levels = compute_levels(&graph);
    let positions = compute_positions(&graph, &levels, &options);
    let (width, height) = canvas_size(&positions, &options);

    let edges = render_edges(&graph, &positions, &options);
    let nodes = render_nodes(&graph, &positions, &options);

    let bg_rect = options.background_color.as_ref().map(|bg| {
        rsx! {
            rect { width: "100%", height: "100%", fill: "{bg}" }
        }
    });

    rsx! {
        svg {
            width: "{width}",
            height: "{height}",
            view_box: "0 0 {width} {height}",
            {bg_rect}
            defs {
                marker {
                    id: "graph-arrowhead",
                    marker_width: "10",
                    marker_height: "7",
                    ref_x: "9",
                    ref_y: "3.5",
                    orient: "auto",
                    polygon { points: "0 0, 10 3.5, 0 7", fill: "#9ca3af" }
                }
            }
            {edges}
            {nodes}
        }
    }
}

/// Compute the hierarchical level (longest path from any source) of every
/// node. Nodes on cycles (or downstream of one) never reach in-degree zero in
/// Kahn's algorithm, so when the queue stalls the lowest-index remaining node
/// is forced to one past its deepest already-placed predecessor and processing
/// continues. This keeps every level finite and deterministic.
fn compute_levels<N, E>(graph: &DiGraph<N, E>) -> HashMap<NodeIndex, usize> {
    let mut in_degree: HashMap<NodeIndex, usize> = HashMap::new();
    let mut queue: VecDeque<NodeIndex> = VecDeque::new();

    for node in graph.node_indices() {
        let degree = graph.edges_directed(node, Direction::Incoming).count();
        in_degree.insert(node, degree);
        if degree == 0 {
            queue.push_back(node);
        }
    }

    let total = graph.node_count();
    let mut processed_count = 0;
    let mut processed: std::collections::HashSet<NodeIndex> =
        std::collections::HashSet::with_capacity(total);
    let mut levels: HashMap<NodeIndex, usize> = HashMap::new();

    while processed_count < total {
        while let Some(node) = queue.pop_front() {
            if !processed.insert(node) {
                continue;
            }
            processed_count += 1;
            let level = levels.get(&node).copied().unwrap_or(0);
            for neighbor in graph.neighbors_directed(node, Direction::Outgoing) {
                if processed.contains(&neighbor) {
                    continue;
                }
                let next = levels.entry(neighbor).or_insert(0);
                *next = (*next).max(level + 1);
                let entry = in_degree.get_mut(&neighbor).expect("valid neighbor");
                *entry = entry.saturating_sub(1);
                if *entry == 0 {
                    queue.push_back(neighbor);
                }
            }
        }
        if processed_count == total {
            break;
        }
        // Stalled on a cycle: force the lowest-index unplaced node.
        let node = graph
            .node_indices()
            .filter(|n| !processed.contains(n))
            .min_by_key(|n| n.index())
            .expect("unprocessed nodes remain");
        let level = graph
            .neighbors_directed(node, Direction::Incoming)
            .filter_map(|p| levels.get(&p).copied())
            .max()
            .map(|l| l + 1)
            .unwrap_or(0);
        levels.insert(node, level);
        in_degree.insert(node, 0);
        queue.push_back(node);
    }

    levels
}

fn compute_positions<N, E>(
    graph: &DiGraph<N, E>,
    levels: &HashMap<NodeIndex, usize>,
    options: &GraphOptions,
) -> HashMap<NodeIndex, NodePos> {
    let mut by_level: HashMap<usize, Vec<NodeIndex>> = HashMap::new();

    for node in graph.node_indices() {
        let level = levels.get(&node).copied().unwrap_or(0);
        by_level.entry(level).or_default().push(node);
    }

    let max_count = by_level.values().map(|v| v.len()).max().unwrap_or(1);
    let level_width = max_count as f32 * options.node_width
        + (max_count.max(1) as f32 - 1.0) * options.node_gap_x;
    let canvas_width = (level_width + 2.0 * options.margin_x).max(options.min_width as f32);

    let level_height = options.node_height + options.level_gap_y;

    let effective_width = canvas_width - 2.0 * options.margin_x;

    let mut positions: HashMap<NodeIndex, NodePos> = HashMap::new();

    for (level, nodes) in &by_level {
        let count = nodes.len();
        let level_w = count as f32 * options.node_width + (count as f32 - 1.0) * options.node_gap_x;
        let start_x =
            options.margin_x + (effective_width - level_w) / 2.0 + options.node_width / 2.0;

        for (idx, node) in nodes.iter().enumerate() {
            let x = start_x + idx as f32 * (options.node_width + options.node_gap_x);
            let y = options.margin_y + options.node_height / 2.0 + *level as f32 * level_height;

            let pos = match options.orientation {
                GraphOrientation::TopToBottom => NodePos { x, y },
                GraphOrientation::LeftToRight => NodePos { x: y, y: x },
            };
            positions.insert(*node, pos);
        }
    }

    positions
}

fn canvas_size(positions: &HashMap<NodeIndex, NodePos>, options: &GraphOptions) -> (u32, u32) {
    let mut bounds_x = 0.0f32;
    let mut bounds_y = 0.0f32;
    for pos in positions.values() {
        bounds_x = bounds_x.max(pos.x + options.node_width / 2.0 + options.margin_x);
        bounds_y = bounds_y.max(pos.y + options.node_height / 2.0 + options.margin_y);
    }
    (
        bounds_x.ceil().max(options.min_width as f32) as u32,
        bounds_y.ceil().max(options.min_height as f32) as u32,
    )
}

fn edge_endpoints(
    src: NodePos,
    tgt: NodePos,
    orientation: GraphOrientation,
    options: &GraphOptions,
) -> (f32, f32, f32, f32) {
    match orientation {
        GraphOrientation::TopToBottom => (
            src.x,
            src.y + options.node_height / 2.0,
            tgt.x,
            tgt.y - options.node_height / 2.0,
        ),
        GraphOrientation::LeftToRight => (
            src.x + options.node_width / 2.0,
            src.y,
            tgt.x - options.node_width / 2.0,
            tgt.y,
        ),
    }
}

fn render_edges<N, E>(
    graph: &DiGraph<N, E>,
    positions: &HashMap<NodeIndex, NodePos>,
    options: &GraphOptions,
) -> Element
where
    E: GraphEdgeLabel + Clone,
{
    let elements: Vec<Element> = graph
        .edge_references()
        .map(|edge| {
            let src = positions[&edge.source()];
            let tgt = positions[&edge.target()];
            let (sx, sy, tx, ty) = edge_endpoints(src, tgt, options.orientation, options);
            let mid_x = (sx + tx) / 2.0;
            let mid_y = (sy + ty) / 2.0;

            let weight = graph.edge_weight(edge.id()).expect("valid edge");
            let stroke = weight.color().unwrap_or_else(|| "#555555".to_string());
            let dash_value = if weight.dashed() { "4,4" } else { "none" };
            let label = weight.label().filter(|l| !l.is_empty());

            rsx! {
                path {
                    d: "M {sx:.1} {sy:.1} C {sx:.1} {mid_y:.1}, {tx:.1} {mid_y:.1}, {tx:.1} {ty:.1}",
                    fill: "none",
                    stroke: "{stroke}",
                    stroke_width: "{options.stroke_width}",
                    marker_end: "url(#graph-arrowhead)",
                    stroke_dasharray: "{dash_value}",
                }
                if let Some(label) = label {
                    text {
                        x: "{mid_x:.1}",
                        y: "{mid_y:.1}",
                        text_anchor: "middle",
                        dominant_baseline: "middle",
                        font_size: "{options.font_size * 0.9}",
                        fill: "{stroke}",
                        "{label}"
                    }
                }
            }
        })
        .collect();

    rsx! {
        {elements.into_iter()}

    }
}

fn render_nodes<N, E>(
    graph: &DiGraph<N, E>,
    positions: &HashMap<NodeIndex, NodePos>,
    options: &GraphOptions,
) -> Element
where
    N: GraphNodeLabel + Clone,
{
    let elements: Vec<Element> = graph
        .node_indices()
        .map(|node| {
            let pos = positions[&node];
            let weight = graph.node_weight(node).expect("valid node");
            let label = weight.label();
            let fill = weight.color().unwrap_or_else(|| "#f8f9fa".to_string());
            let half_w = options.node_width / 2.0;
            let half_h = options.node_height / 2.0;
            let x = pos.x - half_w;
            let y = pos.y - half_h;

            let lines: Vec<&str> = label.split('\n').collect();
            let line_count = lines.len();
            let total_height = options.font_size * line_count as f32 * 1.2;
            let start_y = pos.y - total_height / 2.0 + options.font_size * 0.6;
            let spans: Vec<(String, f32)> = lines
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    let dy = if i == 0 { 0.0 } else { options.font_size * 1.2 };
                    (line.to_string(), dy)
                })
                .collect();

            rsx! {
                rect {
                    x: "{x:.1}",
                    y: "{y:.1}",
                    width: "{options.node_width}",
                    height: "{options.node_height}",
                    rx: "{options.node_radius}",
                    ry: "{options.node_radius}",
                    fill: "{fill}",
                    stroke: "#333333",
                    stroke_width: "{options.stroke_width}",
                }
                text {
                    x: "{pos.x:.1}",
                    y: "{start_y:.1}",
                    text_anchor: "middle",
                    dominant_baseline: "middle",
                    font_size: "{options.font_size}",
                    fill: "#212529",
                    for (line , dy) in spans {
                        tspan { x: "{pos.x:.1}", dy: "{dy:.1}", "{line}" }
                    }
                }
            }
        })
        .collect();

    rsx! {
        {elements.into_iter()}

    }
}
