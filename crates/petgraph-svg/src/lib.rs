//! Render a [`petgraph`] directed graph to a static SVG image.
//!
//! The crate is intentionally small: it takes a [`petgraph::graph::DiGraph`],
//! computes a hierarchical layout, and writes an SVG file. It is aimed at
//! quick dev-time visualisation of dependency graphs rather than publication
//! quality diagrams.
//!
//! # Example
//!
//! ```rust
//! use petgraph::graph::DiGraph;
//! use petgraph_svg::{graph_to_svg, NodeLabel, RenderOptions};
//!
//! let mut graph = DiGraph::<String, ()>::new();
//! let a = graph.add_node("A".to_string());
//! let b = graph.add_node("B".to_string());
//! let c = graph.add_node("C".to_string());
//! graph.add_edge(a, b, ());
//! graph.add_edge(a, c, ());
//!
//! # std::fs::remove_file("/tmp/example.svg").ok();
//! graph_to_svg(&graph, "/tmp/example.svg", &RenderOptions::default()).unwrap();
//! ```

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;

/// Orientation of the hierarchical layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Root nodes at the top, children below.
    TopToBottom,
    /// Root nodes at the left, children to the right.
    LeftToRight,
}

/// A single entry for the graph legend.
#[derive(Debug, Clone)]
pub struct LegendItem {
    /// Text shown next to the sample line.
    pub label: String,
    /// Stroke colour of the sample line.
    pub color: String,
    /// Optional dash-array for the sample line.
    pub dash_array: Option<String>,
}

impl LegendItem {
    /// Convenience constructor for a solid-line legend entry.
    pub fn solid(label: impl Into<String>, color: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            color: color.into(),
            dash_array: None,
        }
    }

    /// Convenience constructor for a dashed-line legend entry.
    pub fn dashed(
        label: impl Into<String>,
        color: impl Into<String>,
        dash_array: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            color: color.into(),
            dash_array: Some(dash_array.into()),
        }
    }
}

/// Options that control the generated SVG.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Minimum width of the SVG canvas in pixels.
    pub min_width: u32,
    /// Minimum height of the SVG canvas in pixels.
    pub min_height: u32,
    /// Width of each node rectangle in pixels.
    pub node_width: f32,
    /// Height of each node rectangle in pixels.
    pub node_height: f32,
    /// Font size for node labels in pixels.
    pub font_size: f32,
    /// Horizontal gap between nodes on the same level in pixels.
    pub node_gap_x: f32,
    /// Vertical gap between levels in pixels.
    pub level_gap_y: f32,
    /// Horizontal margin in pixels.
    pub margin_x: f32,
    /// Vertical margin in pixels.
    pub margin_y: f32,
    /// Layout direction.
    pub orientation: Orientation,
    /// Corner radius for node rectangles in pixels.
    pub node_radius: f32,
    /// Stroke width for node rectangles in pixels.
    pub stroke_width: f32,
    /// Optional SVG background colour. `None` leaves the background transparent.
    pub background_color: Option<String>,
    /// Legend entries drawn in the top-right corner.
    pub legend: Vec<LegendItem>,
    /// Font size for legend text in pixels.
    pub legend_font_size: f32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            min_width: 400,
            min_height: 300,
            node_width: 160.0,
            node_height: 44.0,
            font_size: 12.0,
            node_gap_x: 24.0,
            level_gap_y: 72.0,
            margin_x: 32.0,
            margin_y: 40.0,
            orientation: Orientation::TopToBottom,
            node_radius: 6.0,
            stroke_width: 1.5,
            background_color: None,
            legend: Vec::new(),
            legend_font_size: 11.0,
        }
    }
}

/// Error returned when rendering fails.
#[derive(Debug)]
pub struct DrawError {
    message: String,
}

impl DrawError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for DrawError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for DrawError {}

impl From<io::Error> for DrawError {
    fn from(err: io::Error) -> Self {
        DrawError::new(format!("io error: {err}"))
    }
}

/// Types that can provide a label (and optional colour) for a graph node.
pub trait NodeLabel {
    /// Text label drawn inside the node.
    fn label(&self) -> String;

    /// Optional SVG fill colour. Returning `None` uses the default.
    fn color(&self) -> Option<String> {
        None
    }
}

impl NodeLabel for String {
    fn label(&self) -> String {
        self.clone()
    }
}

impl NodeLabel for &str {
    fn label(&self) -> String {
        (*self).to_string()
    }
}

impl NodeLabel for () {
    fn label(&self) -> String {
        String::new()
    }
}

/// Types that can provide an optional label and colour for a graph edge.
pub trait EdgeLabel {
    /// Optional text label drawn along the edge.
    fn label(&self) -> Option<String> {
        None
    }

    /// Optional SVG stroke colour. Returning `None` uses the default.
    fn color(&self) -> Option<String> {
        None
    }

    /// Optional SVG stroke dash-array (e.g. `"5,5"` for dashed lines).
    fn dash_array(&self) -> Option<String> {
        None
    }
}

impl EdgeLabel for () {}

impl EdgeLabel for String {
    fn label(&self) -> Option<String> {
        Some(self.clone())
    }
}

impl EdgeLabel for &str {
    fn label(&self) -> Option<String> {
        Some((*self).to_string())
    }
}

impl EdgeLabel for (String, String) {
    fn label(&self) -> Option<String> {
        Some(self.0.clone())
    }

    fn color(&self) -> Option<String> {
        Some(self.1.clone())
    }
}

impl EdgeLabel for (String, String, String) {
    fn label(&self) -> Option<String> {
        Some(self.0.clone())
    }

    fn color(&self) -> Option<String> {
        Some(self.1.clone())
    }

    fn dash_array(&self) -> Option<String> {
        Some(self.2.clone())
    }
}

/// Position assigned to a node by the layout pass.
#[derive(Debug, Clone, Copy)]
struct NodePos {
    x: f32,
    y: f32,
}

/// Render `graph` as an SVG image and write it to `path`.
///
/// `graph` should be a directed graph where edges point from a parent to its
/// dependencies (e.g. `goal -> prerequisite`). The layout places sources at
/// the top (or left, depending on [`Orientation`]) and flows downward/rightward.
pub fn graph_to_svg<N, E, P>(
    graph: &DiGraph<N, E>,
    path: P,
    options: &RenderOptions,
) -> Result<(), DrawError>
where
    N: NodeLabel,
    E: EdgeLabel,
    P: AsRef<Path>,
{
    if graph.node_count() == 0 {
        return Err(DrawError::new("cannot render an empty graph"));
    }

    let levels = compute_levels(graph);
    let positions = compute_positions(graph, &levels, options);
    let svg = build_svg(graph, &positions, options);

    let mut file = File::create(path.as_ref())?;
    file.write_all(svg.as_bytes())?;
    Ok(())
}

/// Compute the hierarchical level (longest path from any source) of every node.
fn compute_levels<N, E>(graph: &DiGraph<N, E>) -> HashMap<NodeIndex, usize> {
    let mut in_degree: HashMap<NodeIndex, usize> = HashMap::new();
    let mut topo: Vec<NodeIndex> = Vec::with_capacity(graph.node_count());
    let mut queue: VecDeque<NodeIndex> = VecDeque::new();

    for node in graph.node_indices() {
        let degree = graph.edges_directed(node, Direction::Incoming).count();
        in_degree.insert(node, degree);
        if degree == 0 {
            queue.push_back(node);
        }
    }

    while let Some(node) = queue.pop_front() {
        topo.push(node);
        for neighbor in graph.neighbors_directed(node, Direction::Outgoing) {
            let entry = in_degree.get_mut(&neighbor).expect("valid neighbor");
            *entry -= 1;
            if *entry == 0 {
                queue.push_back(neighbor);
            }
        }
    }

    let mut levels: HashMap<NodeIndex, usize> = HashMap::new();
    for node in &topo {
        let level = levels.get(node).copied().unwrap_or(0);
        for neighbor in graph.neighbors_directed(*node, Direction::Outgoing) {
            let next = levels.entry(neighbor).or_insert(0);
            *next = (*next).max(level + 1);
        }
    }

    levels
}

/// Assign (x, y) coordinates to every node based on its level.
fn compute_positions<N, E>(
    graph: &DiGraph<N, E>,
    levels: &HashMap<NodeIndex, usize>,
    options: &RenderOptions,
) -> HashMap<NodeIndex, NodePos> {
    let mut by_level: HashMap<usize, Vec<NodeIndex>> = HashMap::new();
    let mut max_level = 0;

    for node in graph.node_indices() {
        let level = levels.get(&node).copied().unwrap_or(0);
        by_level.entry(level).or_default().push(node);
        max_level = max_level.max(level);
    }

    let max_count = by_level.values().map(|v| v.len()).max().unwrap_or(1);
    let level_width = max_count as f32 * options.node_width
        + (max_count.max(1) as f32 - 1.0) * options.node_gap_x;
    let canvas_width = (level_width + 2.0 * options.margin_x).max(options.min_width as f32);

    let num_levels = max_level + 1;
    let level_height = options.node_height + options.level_gap_y;
    let canvas_height =
        (num_levels as f32 * level_height + 2.0 * options.margin_y).max(options.min_height as f32);

    // Reserve space for arrowheads and labels at the bottom/right edge.
    let effective_width = canvas_width - 2.0 * options.margin_x;
    let _effective_height = canvas_height - 2.0 * options.margin_y;

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
                Orientation::TopToBottom => NodePos { x, y },
                Orientation::LeftToRight => NodePos { x: y, y: x },
            };
            positions.insert(*node, pos);
        }
    }

    positions
}

/// Build the SVG document string.
fn build_svg<N, E>(
    graph: &DiGraph<N, E>,
    positions: &HashMap<NodeIndex, NodePos>,
    options: &RenderOptions,
) -> String
where
    N: NodeLabel,
    E: EdgeLabel,
{
    let mut bounds_x = 0.0f32;
    let mut bounds_y = 0.0f32;
    for pos in positions.values() {
        bounds_x = bounds_x.max(pos.x + options.node_width / 2.0 + options.margin_x);
        bounds_y = bounds_y.max(pos.y + options.node_height / 2.0 + options.margin_y);
    }

    // The canvas size is derived from the same formula in compute_positions,
    // but we clamp to the actual bounds to avoid excessive whitespace.
    let width = bounds_x.ceil().max(options.min_width as f32);
    let height = bounds_y.ceil().max(options.min_height as f32);

    let mut out = String::new();
    out.push_str(&format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">"##,
    ));
    out.push('\n');

    // Background.
    if let Some(ref bg) = options.background_color {
        out.push_str(&format!(
            r##"  <rect width="100%" height="100%" fill="{bg}"/>"##,
        ));
        out.push('\n');
    }

    // Arrowhead marker.
    out.push_str(
        r##"  <defs>
    <marker id="arrowhead" markerWidth="10" markerHeight="7" refX="9" refY="3.5" orient="auto">
      <polygon points="0 0, 10 3.5, 0 7" fill="#333333"/>
    </marker>
  </defs>"##,
    );
    out.push('\n');

    // Edges.
    for edge in graph.edge_references() {
        let source = edge.source();
        let target = edge.target();
        let src_pos = positions[&source];
        let tgt_pos = positions[&target];

        let (sx, sy, tx, ty) = match options.orientation {
            Orientation::TopToBottom => edge_endpoints_top_bottom(src_pos, tgt_pos, options),
            Orientation::LeftToRight => edge_endpoints_left_right(src_pos, tgt_pos, options),
        };

        let mid_x = (sx + tx) / 2.0;
        let mid_y = (sy + ty) / 2.0;

        let weight = graph.edge_weight(edge.id()).expect("valid edge");
        let stroke = weight.color().unwrap_or_else(|| "#555555".to_string());
        let dash_attr = weight
            .dash_array()
            .map(|d| format!(r##" stroke-dasharray="{}""##, d))
            .unwrap_or_default();

        out.push_str(&format!(
            r##"  <path d="M {sx:.1} {sy:.1} C {sx:.1} {mid_y:.1}, {tx:.1} {mid_y:.1}, {tx:.1} {ty:.1}" fill="none" stroke="{stroke}" stroke-width="{stroke_width}"{dash_attr} marker-end="url(#arrowhead)"/>"##,
            stroke_width = options.stroke_width
        ));
        out.push('\n');

        if let Some(label) = weight.label() {
            if !label.is_empty() {
                out.push_str(&format!(
                    r##"  <text x="{mid_x:.1}" y="{mid_y:.1}" text-anchor="middle" dominant-baseline="middle" font-size="{font_size}" fill="{stroke}">{label}</text>"##,
                    font_size = options.font_size * 0.9,
                    label = escape_xml(&label)
                ));
                out.push('\n');
            }
        }
    }

    // Nodes.
    for node in graph.node_indices() {
        let pos = positions[&node];
        let weight = graph.node_weight(node).expect("valid node");
        let label = weight.label();
        let fill = weight.color().unwrap_or_else(|| "#f8f9fa".to_string());
        let half_w = options.node_width / 2.0;
        let half_h = options.node_height / 2.0;
        let x = pos.x - half_w;
        let y = pos.y - half_h;

        out.push_str(&format!(
            r##"  <rect x="{x:.1}" y="{y:.1}" width="{node_width}" height="{node_height}" rx="{node_radius}" ry="{node_radius}" fill="{fill}" stroke="#333333" stroke-width="{stroke_width}"/>"##,
            node_width = options.node_width,
            node_height = options.node_height,
            node_radius = options.node_radius,
            stroke_width = options.stroke_width
        ));
        out.push('\n');

        let text_y = pos.y + options.font_size * 0.35;
        out.push_str(&format!(
            r##"  <text x="{x:.1}" y="{text_y:.1}" text-anchor="middle" dominant-baseline="middle" font-size="{font_size}" fill="#212529" font-family="sans-serif">{label}</text>"##,
            x = pos.x,
            font_size = options.font_size,
            label = escape_xml(&label)
        ));
        out.push('\n');
    }

    // Legend.
    if !options.legend.is_empty() {
        render_legend(&mut out, width, height, options);
    }

    out.push_str("</svg>\n");
    out
}

/// Render a legend in the top-right corner.
fn render_legend(out: &mut String, width: f32, _height: f32, options: &RenderOptions) {
    let item_height = options.legend_font_size + 8.0;
    let line_width = 24.0f32;
    let line_height = 2.0f32;
    let padding = 10.0f32;
    let box_width = {
        let max_label_width = options
            .legend
            .iter()
            .map(|i| i.label.len() as f32 * options.legend_font_size * 0.6)
            .fold(0.0f32, f32::max);
        line_width + 8.0 + max_label_width + 2.0 * padding
    };
    let box_height = options.legend.len() as f32 * item_height + 2.0 * padding;

    let x = width - box_width - options.margin_x / 2.0;
    let y = options.margin_y / 2.0;

    // Legend background.
    out.push_str(&format!(
        r##"  <rect x="{x:.1}" y="{y:.1}" width="{box_width:.1}" height="{box_height:.1}" rx="4" ry="4" fill="#ffffff" stroke="#cccccc" stroke-width="1" opacity="0.95"/>"##,
    ));
    out.push('\n');

    for (idx, item) in options.legend.iter().enumerate() {
        let item_y = y + padding + idx as f32 * item_height + options.legend_font_size / 2.0;
        let line_x1 = x + padding;
        let line_x2 = line_x1 + line_width;
        let text_x = line_x2 + 8.0;

        let dash_attr = item
            .dash_array
            .as_ref()
            .map(|d| format!(r##" stroke-dasharray="{}""##, escape_xml(d)))
            .unwrap_or_default();

        out.push_str(&format!(
            r##"  <line x1="{line_x1:.1}" y1="{item_y:.1}" x2="{line_x2:.1}" y2="{item_y:.1}" stroke="{color}" stroke-width="{line_height}"{dash_attr}/>"##,
            color = escape_xml(&item.color)
        ));
        out.push('\n');

        out.push_str(&format!(
            r##"  <text x="{text_x:.1}" y="{item_y:.1}" dominant-baseline="middle" font-size="{font_size}" fill="#333333" font-family="sans-serif">{label}</text>"##,
            font_size = options.legend_font_size,
            label = escape_xml(&item.label)
        ));
        out.push('\n');
    }
}

fn edge_endpoints_top_bottom(
    src: NodePos,
    tgt: NodePos,
    options: &RenderOptions,
) -> (f32, f32, f32, f32) {
    let sx = src.x;
    let sy = src.y + options.node_height / 2.0;
    let tx = tgt.x;
    let ty = tgt.y - options.node_height / 2.0;
    (sx, sy, tx, ty)
}

fn edge_endpoints_left_right(
    src: NodePos,
    tgt: NodePos,
    options: &RenderOptions,
) -> (f32, f32, f32, f32) {
    let sx = src.x + options.node_width / 2.0;
    let sy = src.y;
    let tx = tgt.x - options.node_width / 2.0;
    let ty = tgt.y;
    (sx, sy, tx, ty)
}

/// Escape characters that are special in XML text.
fn escape_xml(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '&' => "&amp;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&apos;".to_string(),
            _ => c.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_simple_tree() {
        let mut graph = DiGraph::<String, ()>::new();
        let a = graph.add_node("A".to_string());
        let b = graph.add_node("B".to_string());
        let c = graph.add_node("C".to_string());
        graph.add_edge(a, b, ());
        graph.add_edge(a, c, ());

        let dir = std::env::temp_dir();
        let path = dir.join("petgraph-svg-test-simple.svg");
        graph_to_svg(&graph, &path, &RenderOptions::default()).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("<svg "));
        assert!(content.contains("A"));
        assert!(content.contains("B"));
        assert!(content.contains("C"));
    }

    #[test]
    fn empty_graph_errors() {
        let graph = DiGraph::<String, ()>::new();
        let err = graph_to_svg(
            &graph,
            "/tmp/petgraph-svg-empty.svg",
            &RenderOptions::default(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("empty"));
    }
}
