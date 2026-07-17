//! Blueprint dependency graph data for the scheduler page.
//!
//! This module returns the raw symbolic [`BlueprintGraph`] from `faf_sim`, plus
//! a map of node ids to [`UnitSummary`] so the frontend can render the graph and
//! show unit details on click. All rendering decisions (colors, layers, edge
//! styles, filtering) live in the frontend component.

use std::collections::HashMap;

use faf_sim::units::{role_of, BlueprintGraph, BlueprintLibrary, TechLevel, UnitKind, UnitRole};
use faf_units::{DataIndex, Unit};
use serde::Serialize;

/// Raw blueprint graph bundled with the unit summaries needed by the UI.
#[derive(Debug, Clone, Serialize)]
pub struct BlueprintGraphResponse {
    pub graph: BlueprintGraph,
    /// Unit summaries keyed by graph node id.
    ///
    /// A [`BlueprintGraph`] is intentionally symbolic: each node only knows the
    /// abstract [`UnitKind`], a display name, a role, and a UI category. That is
    /// enough to draw the graph topology, but the scheduler UI also needs
    /// concrete blueprint-level details when a node is clicked: the stable
    /// blueprint id, faction, tech level, browser category, portrait, build
    /// costs, and economic output. Those fields live in [`UnitSummary`].
    ///
    /// We ship the summaries alongside the graph because the frontend does not
    /// have access to the raw `faf-units` [`DataIndex`] and therefore cannot
    /// resolve an abstract `UnitKind` (e.g. `Mex(T1)`) back to a representative
    /// blueprint id on its own. The server uses [`BlueprintLibrary::blueprint_id`]
    /// to pick that representative and then builds a `UnitSummary` from the
    /// index.
    pub summaries: HashMap<String, crate::UnitSummary>,
}

/// Build the raw blueprint graph and attach a unit summary for every shown node.
pub fn blueprint_graph_response(index: &DataIndex) -> BlueprintGraphResponse {
    let library = BlueprintLibrary::new(index.clone());
    let graph = library.build_graph();

    let summaries = graph
        .graph
        .node_weights()
        .filter(|node| should_show(&node.kind))
        .map(|node| {
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
            (id, summary)
        })
        .collect();

    BlueprintGraphResponse { graph, summaries }
}

/// Stable unique identifier for a graph node.
pub fn node_id(kind: &UnitKind) -> String {
    match kind {
        UnitKind::Unique(id) => id.0.clone(),
        _ => format!("{kind:?}"),
    }
}

/// Human-readable label for a graph node.
pub fn node_label(kind: &UnitKind) -> String {
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

/// Whether a unit kind should appear in the economic/builder subgraph.
pub fn should_show(kind: &UnitKind) -> bool {
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
