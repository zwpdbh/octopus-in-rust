//! Concrete blueprint dependency graph for the map popup.
//!
//! Nodes are real blueprints (e.g. `UEL0001` UEF ACU, `UEL0105` T1 UEF
//! engineer). Built-by edges are derived from the `BUILTBY*` category tags in
//! the unit index; upgrade edges are derived from tier chains within the same
//! (faction, role). The frontend renders faction subgraphs of this graph.
//!
//! The abstract symbolic [`faf_sim::units::BlueprintGraph`] used by the
//! scheduler algorithm is a separate model and is untouched.

use std::collections::HashMap;

use faf_sim::units::{TechLevel, UnitId, UnitKind};
use faf_units::{DataIndex, Unit};
use serde::Serialize;

const FACTIONS: [&str; 4] = ["UEF", "Cybran", "Aeon", "Seraphim"];

/// Economic/builder role of a concrete node; drives color and upgrade chains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EconRole {
    Commander,
    Engineer,
    Factory,
    Mex,
    Pgen,
    MassStorage,
    EnergyStorage,
    Experimental,
}

impl EconRole {
    /// Producer priority used to drop same-tier bidirectional build edges.
    fn producer_priority(self) -> i32 {
        match self {
            EconRole::Commander => 4,
            EconRole::Factory => 3,
            EconRole::Engineer => 2,
            _ => 1,
        }
    }
}

/// A concrete unit node in the relationship graph.
#[derive(Debug, Clone, Serialize)]
pub struct ConcreteGraphNode {
    /// Blueprint id, e.g. "UEL0105".
    pub id: String,
    pub display_name: String,
    pub faction: String,
    pub tech: String,
    pub role: EconRole,
    /// Dagre layer: ACU=0, T1=1, T2=2, T3=3, Experimental=4.
    pub layer: i32,
    /// Abstract kind this concrete unit maps to (needed by schedule requests).
    pub kind: UnitKind,
}

/// The kind of a directed edge between two concrete units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConcreteEdgeKind {
    BuiltBy,
    UpgradesInto,
}

/// A directed edge from `source` (builder / lower tier) to `target`.
#[derive(Debug, Clone, Serialize)]
pub struct ConcreteGraphEdge {
    pub source: String,
    pub target: String,
    pub kind: ConcreteEdgeKind,
}

/// The full concrete relationship graph plus per-node unit summaries.
#[derive(Debug, Clone, Serialize)]
pub struct ConcreteGraphResponse {
    pub nodes: Vec<ConcreteGraphNode>,
    pub edges: Vec<ConcreteGraphEdge>,
    pub summaries: HashMap<String, crate::UnitSummary>,
}

/// Build the concrete relationship graph from the raw unit index.
pub fn concrete_graph_response(index: &DataIndex) -> ConcreteGraphResponse {
    let nodes = collect_nodes(index);
    let edges = collect_edges(index, &nodes);
    let summaries = nodes
        .iter()
        .filter_map(|node| {
            index
                .find_unit(&node.id)
                .map(|unit| (node.id.clone(), unit_summary(unit)))
        })
        .collect();
    ConcreteGraphResponse {
        nodes,
        edges,
        summaries,
    }
}

fn collect_nodes(index: &DataIndex) -> Vec<ConcreteGraphNode> {
    let mut nodes: Vec<ConcreteGraphNode> = index.units.iter().filter_map(classify_node).collect();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    nodes
}

/// Classify a raw unit into a concrete graph node, or `None` if it is not part
/// of the economic/builder relationship graph.
fn classify_node(unit: &Unit) -> Option<ConcreteGraphNode> {
    let faction = unit.faction()?;
    if !FACTIONS.contains(&faction) {
        return None;
    }

    let has = |cat: &str| unit.has_category(cat);
    let tier = unit.tech_level();

    let (role, kind): (EconRole, UnitKind) = if has("COMMAND") {
        (EconRole::Commander, UnitKind::Commander)
    } else if tier == Some("TECH4") || tier == Some("EXPERIMENTAL") {
        let kind = if unit.id == "UEL0401" {
            UnitKind::Experimental
        } else {
            UnitKind::Unique(UnitId(unit.id.clone()))
        };
        (EconRole::Experimental, kind)
    } else if has("ENGINEER") && has("MOBILE") && has("LAND") && !has("SUBCOMMANDER") {
        (EconRole::Engineer, UnitKind::Engineer(parse_tech(tier)?))
    } else if has("FACTORY")
        && has("STRUCTURE")
        && has("LAND")
        && !has("SUPPORTFACTORY")
        && !has("GATE")
    {
        (EconRole::Factory, UnitKind::Factory(parse_tech(tier)?))
    } else if has("MASSEXTRACTION") && has("STRUCTURE") {
        (EconRole::Mex, UnitKind::Mex(parse_tech(tier)?))
    } else if has("ENERGYPRODUCTION") && has("STRUCTURE") && !has("HYDROCARBON") {
        (EconRole::Pgen, UnitKind::Pgen(parse_tech(tier)?))
    } else if has("MASSSTORAGE") && has("STRUCTURE") {
        (
            EconRole::MassStorage,
            UnitKind::Unique(UnitId(unit.id.clone())),
        )
    } else if has("ENERGYSTORAGE") && has("STRUCTURE") {
        (EconRole::EnergyStorage, UnitKind::EnergyStorage)
    } else {
        return None;
    };

    Some(ConcreteGraphNode {
        id: unit.id.clone(),
        display_name: unit.name().unwrap_or(&unit.id).to_string(),
        faction: faction.to_string(),
        tech: tier.unwrap_or("").to_string(),
        role,
        layer: layer_of(role, tier),
        kind,
    })
}

fn parse_tech(tech: Option<&str>) -> Option<TechLevel> {
    match tech {
        Some("TECH1") => Some(TechLevel::T1),
        Some("TECH2") => Some(TechLevel::T2),
        Some("TECH3") => Some(TechLevel::T3),
        _ => None,
    }
}

fn layer_of(role: EconRole, tech: Option<&str>) -> i32 {
    match role {
        EconRole::Commander => 0,
        EconRole::EnergyStorage => 1,
        EconRole::Experimental => 4,
        _ => match tech {
            Some("TECH1") => 1,
            Some("TECH2") => 2,
            Some("TECH3") => 3,
            _ => 0,
        },
    }
}

fn collect_edges(index: &DataIndex, nodes: &[ConcreteGraphNode]) -> Vec<ConcreteGraphEdge> {
    let node_ids: std::collections::HashSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();
    let by_role: HashMap<(&str, EconRole, Option<TechLevel>), &ConcreteGraphNode> = nodes
        .iter()
        .map(|n| ((n.faction.as_str(), n.role, tech_of_kind(&n.kind)), n))
        .collect();

    let mut edges = Vec::new();

    for node in nodes {
        let Some(unit) = index.find_unit(&node.id) else {
            continue;
        };
        for tag in unit.categories.iter().map(String::as_str) {
            let source = match tag {
                "BUILTBYCOMMANDER"
                | "BUILTBYTIER1COMMANDER"
                | "BUILTBYTIER2COMMANDER"
                | "BUILTBYTIER3COMMANDER" => {
                    by_role.get(&(node.faction.as_str(), EconRole::Commander, None))
                }
                "BUILTBYTIER1ENGINEER" => by_role.get(&(
                    node.faction.as_str(),
                    EconRole::Engineer,
                    Some(TechLevel::T1),
                )),
                "BUILTBYTIER2ENGINEER" => by_role.get(&(
                    node.faction.as_str(),
                    EconRole::Engineer,
                    Some(TechLevel::T2),
                )),
                "BUILTBYTIER3ENGINEER" => by_role.get(&(
                    node.faction.as_str(),
                    EconRole::Engineer,
                    Some(TechLevel::T3),
                )),
                "BUILTBYTIER1FACTORY" => by_role.get(&(
                    node.faction.as_str(),
                    EconRole::Factory,
                    Some(TechLevel::T1),
                )),
                "BUILTBYTIER2FACTORY" => by_role.get(&(
                    node.faction.as_str(),
                    EconRole::Factory,
                    Some(TechLevel::T2),
                )),
                "BUILTBYTIER3FACTORY" => by_role.get(&(
                    node.faction.as_str(),
                    EconRole::Factory,
                    Some(TechLevel::T3),
                )),
                _ => None,
            };
            let Some(source) = source else { continue };
            if source.id == node.id {
                continue;
            }
            // Layout-oriented pruning: only show the minimal honest builder for
            // each target. Higher-tier builders can also construct lower-tier
            // targets in-game, but drawing those edges buries the graph under
            // fan-out; the tech chain keeps reachability visible.
            //   - builders never point to lower-tier targets
            //   - the unupgraded ACU only builds T1 targets
            if source.layer > node.layer {
                continue;
            }
            if source.role == EconRole::Commander && node.layer > 1 {
                continue;
            }
            edges.push(ConcreteGraphEdge {
                source: source.id.clone(),
                target: node.id.clone(),
                kind: ConcreteEdgeKind::BuiltBy,
            });
        }
    }

    // Same-tier bidirectional "built by" edges (e.g. Factory T1 <-> Eng T1):
    // keep the producer -> product direction only.
    let by_id: HashMap<&str, &ConcreteGraphNode> =
        nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let edge_set: std::collections::HashSet<(String, String)> = edges
        .iter()
        .map(|e| (e.source.clone(), e.target.clone()))
        .collect();
    edges.retain(|e| {
        if !edge_set.contains(&(e.target.clone(), e.source.clone())) {
            return true;
        }
        let source = by_id[e.source.as_str()];
        let target = by_id[e.target.as_str()];
        if source.layer != target.layer {
            return true;
        }
        source.role.producer_priority() >= target.role.producer_priority()
    });

    // Upgrade edges: tier chains within (faction, Factory|Mex|Pgen).
    for role in [EconRole::Factory, EconRole::Mex, EconRole::Pgen] {
        for faction in FACTIONS {
            for (lower, higher) in [
                (TechLevel::T1, TechLevel::T2),
                (TechLevel::T2, TechLevel::T3),
            ] {
                let from = by_role.get(&(faction, role, Some(lower)));
                let to = by_role.get(&(faction, role, Some(higher)));
                if let (Some(from), Some(to)) = (from, to) {
                    edges.push(ConcreteGraphEdge {
                        source: from.id.clone(),
                        target: to.id.clone(),
                        kind: ConcreteEdgeKind::UpgradesInto,
                    });
                }
            }
        }
    }

    // Final dedup: for the same (source, target) prefer UpgradesInto over BuiltBy.
    let mut best: HashMap<(String, String), ConcreteGraphEdge> = HashMap::new();
    for edge in edges {
        let key = (edge.source.clone(), edge.target.clone());
        best.entry(key)
            .and_modify(|existing| {
                if existing.kind == ConcreteEdgeKind::BuiltBy
                    && edge.kind == ConcreteEdgeKind::UpgradesInto
                {
                    *existing = edge.clone();
                }
            })
            .or_insert(edge);
    }
    let mut deduped: Vec<ConcreteGraphEdge> = best.into_values().collect();
    deduped
        .retain(|e| node_ids.contains(e.source.as_str()) && node_ids.contains(e.target.as_str()));
    deduped.sort_by(|a, b| (&a.source, &a.target).cmp(&(&b.source, &b.target)));
    deduped
}

fn tech_of_kind(kind: &UnitKind) -> Option<TechLevel> {
    match kind {
        UnitKind::Engineer(t) | UnitKind::Factory(t) | UnitKind::Mex(t) | UnitKind::Pgen(t) => {
            Some(*t)
        }
        _ => None,
    }
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
