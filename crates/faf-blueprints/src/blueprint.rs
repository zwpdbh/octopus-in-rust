//! ECS-backed blueprint library for unit knowledge.
//!
//! [`BlueprintLibrary`] is a self-contained, ECS-backed model of the units that
//! matter for build-order optimization. It is built once from the raw
//! `faf-units` index and then used without string lookups by the simulator and
//! planners.
//!
//! Each unit definition is represented as a blueprint entity in a dedicated
//! Bevy `World`. Static attributes are stored as components; see the
//! `components` module for the full list.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use bevy_ecs::prelude::*;
use faf_units::FafUnitIndex;

use crate::unit_eco::{AdjacencyBonus, UnitEcoStats};

use super::build;
use super::components::{
    attributes::{
        BlueprintBundle, BlueprintId, DisplayName, FactionComp, UnitKindComp, UnitRoleComp,
    },
    relationships::{BuiltBy, CapsInto, UpgradesInto},
};
use super::graph::{BlueprintEdge, BlueprintGraph};
use super::types::{
    category_of, matches_tech_level, role_of, tech_level_of, BuildRule, Faction, TechLevel,
    UnitCategory, UnitCost, UnitKind, UnitRole,
};

/// Unified repository of unit knowledge backed by a Bevy ECS blueprint world.
///
/// `BlueprintLibrary` is self-contained: after construction it no longer
/// references the raw `DataIndex`. All build/upgrade rules are explicit recipes
/// rather than derived string-category graphs.
#[derive(Debug)]
pub struct FAFBlueprint {
    world: World,
    kind_to_entity: HashMap<UnitKind, Entity>,
    /// Runtime economic stats for every known blueprint, keyed by abstract kind.
    ///
    /// This table is the boundary between the symbolic blueprint world and the
    /// numeric simulation runtime. It is populated once during construction and
    /// then treated as read-only.
    blueprint_stats_table: HashMap<UnitKind, UnitEcoStats>,
}

impl FAFBlueprint {
    /// Build the repository from a raw unit index.
    pub fn new(units_json_file: PathBuf) -> anyhow::Result<Self> {
        let index = FafUnitIndex::new(units_json_file)?;
        FAFBlueprint::from_index(index)
    }

    /// Build the repository from the default FAF units JSON shipped with the
    /// workspace.
    pub fn default() -> anyhow::Result<Self> {
        let index = FafUnitIndex::default()?;
        FAFBlueprint::from_index(index)
    }

    fn from_index(index: FafUnitIndex) -> anyhow::Result<Self> {
        let mut world = World::new();
        let mut kind_to_entity: HashMap<UnitKind, Entity> = HashMap::new();
        let mut blueprint_stats_table: HashMap<UnitKind, UnitEcoStats> = HashMap::new();
        let builds = Self::hardcoded_builds();
        let upgrades = Self::hardcoded_upgrades();
        let caps = Self::hardcoded_caps();

        // Track which common kinds have already been fixed to their canonical
        // UEF blueprint. Non-canonical duplicates are ignored once the canonical
        // definition has been stored.
        let mut canonical_kinds: HashSet<UnitKind> = HashSet::new();

        for unit in &index.units {
            let Some(bundle) = build::blueprint_bundle(unit) else {
                continue;
            };
            let kind = bundle.kind.0.clone();

            if build::is_common_kind(&kind) {
                let is_canonical = build::is_canonical_for_kind(unit, &kind);
                let already_canonical = canonical_kinds.contains(&kind);

                // Keep an earlier non-canonical entry only until we find the
                // canonical one; afterwards ignore further duplicates.
                if kind_to_entity.contains_key(&kind) && !is_canonical {
                    continue;
                }

                if is_canonical {
                    canonical_kinds.insert(kind.clone());
                } else if already_canonical {
                    continue;
                }
            }

            let entity = world.spawn(bundle).id();
            if let Some(tech) = super::types::tech_level_of(&kind) {
                world
                    .entity_mut(entity)
                    .insert(super::components::attributes::TechLevelComp(tech));
            }
            let stats = build::unit_eco_stats(unit, &kind);
            kind_to_entity.insert(kind.clone(), entity);
            blueprint_stats_table.insert(kind, stats);
        }

        // Synthetic definitions for capped mass extractors. These do not exist
        // as raw blueprints; they represent a T2/T3 mex surrounded by four mass
        // storages. The base `ProductionPerSecondMass` matches the underlying
        // mex; the adjacency bonus is applied at runtime based on the number of
        // fully-surrounded sides.
        for (tech, base_mex, blueprint_id, display_name, maintenance) in [
            (
                TechLevel::T2,
                UnitKind::Mex(TechLevel::T2),
                "UEB1202+CAPPED",
                "Capped T2 Mass Extractor",
                9.0,
            ),
            (
                TechLevel::T3,
                UnitKind::Mex(TechLevel::T3),
                "UEB1302+CAPPED",
                "Capped T3 Mass Extractor",
                54.0,
            ),
        ] {
            let base_mass = blueprint_stats_table
                .get(&base_mex)
                .map(|s| s.production_per_second_mass)
                .unwrap_or(0.0);
            let kind = UnitKind::CapMex(tech);
            let stats = UnitEcoStats {
                build_power: 0.0,
                mass_cost: 800.0,
                energy_cost: 6000.0,
                build_time: 1000.0,
                // 4 adjacent mass storages: base * (1 + 4 * 0.125) = base * 1.5
                production_per_second_mass: base_mass,
                production_per_second_energy: 0.0,
                maintenance_consumption_per_second_energy: maintenance,
                mass_storage: 2000.0,
                energy_storage: 0.0,
                adjacency: AdjacencyBonus {
                    mass_storage_sides: 4,
                    ..Default::default()
                },
                unit_id: Some(blueprint_id.to_string()),
            };
            let entity = world
                .spawn(BlueprintBundle {
                    blueprint_id: BlueprintId(blueprint_id.to_string()),
                    kind: UnitKindComp(kind.clone()),
                    role: UnitRoleComp(UnitRole::MassExtractor),
                    faction: FactionComp(Faction::Common),
                    display_name: DisplayName(display_name.to_string()),
                })
                .id();
            kind_to_entity.insert(kind.clone(), entity);
            blueprint_stats_table.insert(kind, stats);
        }

        // Synthetic definition for the T4 experimental tier.
        // The UEF Fatboy is used as the canonical representative.
        if let Some(unit) = index.find_unit("UEL0401") {
            let experimental_stats = build::unit_eco_stats(unit, &UnitKind::Experimental);
            let experimental_entity = world
                .spawn(BlueprintBundle {
                    blueprint_id: BlueprintId("UEL0401".to_string()),
                    kind: UnitKindComp(UnitKind::Experimental),
                    role: UnitRoleComp(UnitRole::Experimental),
                    faction: FactionComp(Faction::Common),
                    display_name: DisplayName("Experimental Unit".to_string()),
                })
                .id();
            kind_to_entity.insert(UnitKind::Experimental, experimental_entity);
            blueprint_stats_table.insert(UnitKind::Experimental, experimental_stats);
        }

        // Attach build, upgrade, and cap rules to their source/target entities.
        //
        // - `BuiltBy` goes on the target entity (who can build it).
        // - `UpgradesInto` and `CapsInto` go on the source entity (what it can
        //   become).
        //
        // The source unit's own build rate drives the transformation in the
        // simulator; no external builder is encoded here because FAF structures
        // can upgrade and cap themselves without assistance.
        for (target, rule) in &builds {
            if let Some(&entity) = kind_to_entity.get(target) {
                world.entity_mut(entity).insert(BuiltBy {
                    prereq: rule.prereq.clone(),
                    builders: rule.builders.clone(),
                });
            }
        }
        for (from, target) in &upgrades {
            if let Some(&entity) = kind_to_entity.get(from) {
                world
                    .entity_mut(entity)
                    .insert(UpgradesInto(target.clone()));
            }
        }
        for (from, target) in &caps {
            if let Some(&entity) = kind_to_entity.get(from) {
                world.entity_mut(entity).insert(CapsInto(target.clone()));
            }
        }

        // Faction-unique units (experimentals, game-enders, strategic weapons)
        // are built by T3 engineers.
        for (kind, &entity) in &kind_to_entity {
            if matches!(kind, UnitKind::Unique(_)) {
                world
                    .entity_mut(entity)
                    .insert(build::unique_unit_build_rule());
            }
        }

        Ok(Self {
            world,
            kind_to_entity,
            blueprint_stats_table,
        })
    }

    /// Entity handle for the blueprint of `kind`, if one exists.
    pub fn entity_for_kind(&self, kind: &UnitKind) -> Option<Entity> {
        self.kind_to_entity.get(kind).copied()
    }

    /// Functional role for a unit kind.
    pub fn role(&self, kind: &UnitKind) -> UnitRole {
        role_of(kind)
    }

    /// UI category for a unit kind.
    pub fn category(&self, kind: &UnitKind) -> UnitCategory {
        category_of(kind)
    }

    /// All unit kinds known to the library.
    pub fn all_kinds(&self) -> Vec<UnitKind> {
        let mut kinds: Vec<UnitKind> = self.kind_to_entity.keys().cloned().collect();
        kinds.sort();
        kinds
    }

    /// All unit kinds whose abstract classification matches the given tech level.
    ///
    /// For example, `kinds_with_tech_level(TechLevel::T2)` returns T2 engineers,
    /// factories, mass extractors, and power generators.
    pub fn kinds_with_tech_level(&self, tech: TechLevel) -> Vec<UnitKind> {
        self.kind_to_entity
            .keys()
            .filter(|kind| matches_tech_level(kind, tech))
            .cloned()
            .collect()
    }

    /// Blueprint id for a unit kind, if one exists.
    ///
    /// Common units map to their canonical UEF representative; unique units
    /// return their own blueprint id.
    pub fn blueprint_id(&self, kind: &UnitKind) -> Option<String> {
        match kind {
            UnitKind::Unique(id) => Some(id.0.clone()),
            UnitKind::CapMex(TechLevel::T2) => Some("UEB1202".to_string()),
            UnitKind::CapMex(TechLevel::T3) => Some("UEB1302".to_string()),
            UnitKind::CapMex(_) => None,
            _ => build::canonical_blueprint_id(kind).map(|s| s.to_string()),
        }
    }

    /// Human-readable name for a unit kind.
    pub fn display_name(&self, kind: &UnitKind) -> String {
        self.entity_for_kind(kind)
            .and_then(|e| self.world.entity(e).get::<DisplayName>())
            .map(|d| d.0.clone())
            .unwrap_or_else(|| format!("{:?}", kind))
    }

    /// Build cost for a unit kind, if it can be built at all.
    pub fn unit_build_cost(&self, kind: &UnitKind) -> Option<UnitCost> {
        self.blueprint_stats_table.get(kind).map(|stats| UnitCost {
            mass: stats.mass_cost,
            energy: stats.energy_cost,
            build_time: stats.build_time,
        })
    }

    /// True if `builder` is one of the legal builders for `target`.
    pub fn can_build(&self, builder: &UnitKind, target: &UnitKind) -> bool {
        self.build_rule(target)
            .map(|r| r.builders.contains(builder))
            .unwrap_or(false)
    }

    /// Return every unit kind that `builder` is allowed to build.
    pub fn buildable_by(&self, builder: &UnitKind) -> Vec<UnitKind> {
        self.kind_to_entity
            .iter()
            .filter_map(|(kind, &entity)| {
                self.world
                    .entity(entity)
                    .get::<BuiltBy>()
                    .filter(|rule| rule.builders.contains(builder))
                    .map(|_| kind.clone())
            })
            .collect()
    }

    /// Return the legal builder kinds for a target.
    pub fn builders_for(&self, target: &UnitKind) -> Vec<UnitKind> {
        self.build_rule(target)
            .map(|r| r.builders.clone())
            .unwrap_or_default()
    }

    /// Return the build rule for a target, if any.
    pub fn build_rule(&self, target: &UnitKind) -> Option<&BuiltBy> {
        let entity = self.entity_for_kind(target)?;
        self.world.entity(entity).get::<BuiltBy>()
    }

    /// All unit kinds that can be built, optionally filtered to a tech tier.
    pub fn target_blueprints(&self, tech: Option<TechLevel>) -> HashSet<UnitKind> {
        self.kind_to_entity
            .iter()
            .filter(|(kind, _)| tech.map_or(true, |t| matches_tech_level(kind, t)))
            .filter_map(|(kind, &entity)| {
                self.world
                    .entity(entity)
                    .get::<BuiltBy>()
                    .map(|_| kind.clone())
            })
            .collect()
    }

    /// Return the single upgrade target for a source unit kind, if any.
    ///
    /// The target is reached by upgrading the source unit. The upgrade's build
    /// power comes from the source unit itself, not from a separate builder
    /// encoded in the blueprint data.
    pub fn upgrade_target(&self, from: &UnitKind) -> Option<UnitKind> {
        self.entity_for_kind(from)
            .and_then(|e| self.world.entity(e).get::<UpgradesInto>())
            .map(|r| r.0.clone())
    }

    /// Return the capped variant of a unit kind, if one exists.
    pub fn cap_target(&self, from: &UnitKind) -> Option<UnitKind> {
        self.entity_for_kind(from)
            .and_then(|e| self.world.entity(e).get::<CapsInto>())
            .map(|r| r.0.clone())
    }

    /// True if the unit has a registered upgrade target.
    pub fn is_upgradeable(&self, kind: &UnitKind) -> bool {
        self.upgrade_target(kind).is_some()
    }

    /// True if the unit has a registered cap target.
    pub fn is_cappable(&self, kind: &UnitKind) -> bool {
        self.cap_target(kind).is_some()
    }

    /// All unit kinds that can act as builders, optionally filtered to a tech tier.
    pub fn builder_blueprints(&self, tech: Option<TechLevel>) -> HashSet<UnitKind> {
        self.blueprint_stats_table
            .iter()
            .filter(|(kind, stats)| {
                stats.build_power > 0.0 && tech.map_or(true, |t| matches_tech_level(kind, t))
            })
            .map(|(kind, _)| kind.clone())
            .collect()
    }

    /// Build power for a unit kind.
    pub fn build_power(&self, kind: &UnitKind) -> f64 {
        self.blueprint_stats_table
            .get(kind)
            .map(|s| s.build_power)
            .unwrap_or(0.0)
    }

    /// Mass production per second for a unit kind, including adjacency bonuses.
    pub fn production_per_second_mass(&self, kind: &UnitKind) -> f64 {
        self.blueprint_stats_table
            .get(kind)
            .map(|s| s.production_per_second_mass * s.adjacency.mass_production_multiplier())
            .unwrap_or(0.0)
    }

    /// Energy production per second for a unit kind, including adjacency bonuses.
    pub fn production_per_second_energy(&self, kind: &UnitKind) -> f64 {
        self.blueprint_stats_table
            .get(kind)
            .map(|s| s.production_per_second_energy * s.adjacency.energy_production_multiplier())
            .unwrap_or(0.0)
    }

    /// Energy maintenance consumption per second for a unit kind.
    pub fn maintenance_consumption_per_second_energy(&self, kind: &UnitKind) -> f64 {
        self.blueprint_stats_table
            .get(kind)
            .map(|s| s.maintenance_consumption_per_second_energy)
            .unwrap_or(0.0)
    }

    /// Mass storage capacity for a unit kind.
    pub fn mass_storage(&self, kind: &UnitKind) -> f64 {
        self.blueprint_stats_table
            .get(kind)
            .map(|s| s.mass_storage)
            .unwrap_or(0.0)
    }

    /// Energy storage capacity for a unit kind.
    pub fn energy_storage(&self, kind: &UnitKind) -> f64 {
        self.blueprint_stats_table
            .get(kind)
            .map(|s| s.energy_storage)
            .unwrap_or(0.0)
    }

    /// Full economic descriptor for a unit kind, if it is defined.
    ///
    /// This includes base production, storage, maintenance, cost, build time,
    /// and adjacency metadata. It is the same shape used by the runtime for
    /// build-task targets and builders.
    pub fn unit_eco_stats(&self, kind: &UnitKind) -> Option<UnitEcoStats> {
        self.blueprint_stats_table.get(kind).cloned()
    }

    /// Convert a blueprint into the flat runtime economic representation.
    ///
    /// `as_builder` controls whether cost/storage fields are zeroed out, matching
    /// the old `unit_as_builder` / `unit_as_target` split.
    pub fn to_unit_eco_stats(&self, kind: &UnitKind, as_builder: bool) -> Option<UnitEcoStats> {
        let mut stats = self.blueprint_stats_table.get(kind).cloned()?;

        if as_builder {
            stats.mass_cost = 0.0;
            stats.energy_cost = 0.0;
            stats.build_time = 0.0;
            stats.production_per_second_mass = 0.0;
            stats.production_per_second_energy = 0.0;
            stats.mass_storage = 0.0;
            stats.energy_storage = 0.0;
        } else {
            stats.build_power = 0.0;
        }

        Some(stats)
    }

    /// Build the symbolic build/upgrade/cap graph for visualization and planning.
    pub fn build_graph(&self) -> BlueprintGraph {
        let mut graph = BlueprintGraph::new();
        let mut indices = HashMap::new();

        // First pass: add every known unit kind as a node so that edges can
        // reference their endpoints by index.
        for (kind, &entity) in &self.kind_to_entity {
            let entity_ref = self.world.entity(entity);
            let display_name = entity_ref
                .get::<DisplayName>()
                .map(|d| d.0.clone())
                .unwrap_or_else(|| format!("{:?}", kind));

            let node = super::graph::BlueprintNode {
                kind: kind.clone(),
                display_name,
                role: role_of(kind),
                category: category_of(kind),
            };
            let idx = graph.add_node(node);
            indices.insert(kind.clone(), idx);
        }

        // Second pass: add build and upgrade edges.
        for (kind, &entity) in &self.kind_to_entity {
            let entity_ref = self.world.entity(entity);
            let target_idx = *indices
                .get(kind)
                .expect("every kind was assigned a node index in the first pass");

            if let Some(rule) = entity_ref.get::<BuiltBy>() {
                for builder in &rule.builders {
                    if let Some(&builder_idx) = indices.get(builder) {
                        // Drop same-tier edges where a lower-priority producer
                        // would otherwise point to a higher-priority producer.
                        // This prevents Factory T1 <-> Eng T1 style bidirectional
                        // cycles in the symbolic graph.
                        if same_tier_lower_priority_builder(builder, kind) {
                            continue;
                        }
                        graph.add_edge(
                            builder_idx,
                            target_idx,
                            BlueprintEdge::BuiltBy {
                                prereq: rule.prereq.clone(),
                            },
                        );
                    }
                }
            }

            if let Some(upgrades) = entity_ref.get::<UpgradesInto>() {
                if let Some(&to_idx) = indices.get(&upgrades.0) {
                    graph.add_edge(target_idx, to_idx, BlueprintEdge::UpgradesInto);
                }
            }

            if let Some(cap) = entity_ref.get::<CapsInto>() {
                if let Some(&to_idx) = indices.get(&cap.0) {
                    graph.add_edge(target_idx, to_idx, BlueprintEdge::CapsInto);
                }
            }
        }

        graph
    }

    /// Hardcoded build rules for the common economic/builder units.
    fn hardcoded_builds() -> HashMap<UnitKind, BuildRule> {
        let mut m: HashMap<UnitKind, BuildRule> = HashMap::new();

        // Commander is given at game start; it is not built.
        m.insert(
            UnitKind::Commander,
            BuildRule {
                prereq: None,
                builders: vec![],
            },
        );

        // Factories.
        for (tech, prereq, builders) in [
            (
                TechLevel::T1,
                None,
                vec![UnitKind::Commander, UnitKind::Engineer(TechLevel::T1)],
            ),
            (
                TechLevel::T2,
                Some(UnitKind::Factory(TechLevel::T1)),
                vec![
                    UnitKind::Commander,
                    UnitKind::Engineer(TechLevel::T2),
                    UnitKind::Engineer(TechLevel::T3),
                ],
            ),
            (
                TechLevel::T3,
                Some(UnitKind::Factory(TechLevel::T2)),
                vec![UnitKind::Commander, UnitKind::Engineer(TechLevel::T3)],
            ),
        ] {
            m.insert(UnitKind::Factory(tech), BuildRule { prereq, builders });
        }

        // Engineers are built by factories of the same tier.
        for tech in [TechLevel::T1, TechLevel::T2, TechLevel::T3] {
            m.insert(
                UnitKind::Engineer(tech),
                BuildRule {
                    prereq: Some(UnitKind::Factory(tech)),
                    builders: vec![UnitKind::Factory(tech)],
                },
            );
        }

        // T1 economy structures are built by the commander or T1 engineers.
        for kind in [UnitKind::Mex(TechLevel::T1), UnitKind::Pgen(TechLevel::T1)] {
            m.insert(
                kind.clone(),
                BuildRule {
                    prereq: None,
                    builders: vec![UnitKind::Commander, UnitKind::Engineer(TechLevel::T1)],
                },
            );
        }

        // Higher-tier economy structures are built by engineers of the same
        // tier (new construction, not upgrade).
        for tech in [TechLevel::T2, TechLevel::T3] {
            for kind in [UnitKind::Mex(tech), UnitKind::Pgen(tech)] {
                m.insert(
                    kind.clone(),
                    BuildRule {
                        prereq: Some(UnitKind::Factory(tech)),
                        builders: vec![UnitKind::Engineer(tech)],
                    },
                );
            }
        }

        // Energy storage can be built by any engineer tier once an engineer exists.
        m.insert(
            UnitKind::EnergyStorage,
            BuildRule {
                prereq: None,
                builders: vec![
                    UnitKind::Engineer(TechLevel::T1),
                    UnitKind::Engineer(TechLevel::T2),
                    UnitKind::Engineer(TechLevel::T3),
                ],
            },
        );

        // T4 experimental units require a T3 factory and are built by T3 engineers.
        m.insert(
            UnitKind::Experimental,
            BuildRule {
                prereq: Some(UnitKind::Factory(TechLevel::T3)),
                builders: vec![UnitKind::Engineer(TechLevel::T3)],
            },
        );

        m
    }

    /// Hardcoded upgrade targets for the common economic units.
    ///
    /// This table is the authoritative source for upgrade relationships because
    /// the raw FAF unit index does not explicitly name upgrade targets. It is
    /// converted into `UpgradesInto` ECS components after all blueprint entities
    /// have been spawned.
    fn hardcoded_upgrades() -> HashMap<UnitKind, UnitKind> {
        let mut m: HashMap<UnitKind, UnitKind> = HashMap::new();

        // Mass extractors: T1 -> T2 -> T3.
        m.insert(UnitKind::Mex(TechLevel::T1), UnitKind::Mex(TechLevel::T2));
        m.insert(UnitKind::Mex(TechLevel::T2), UnitKind::Mex(TechLevel::T3));
        m.insert(
            UnitKind::CapMex(TechLevel::T2),
            UnitKind::CapMex(TechLevel::T3),
        );

        // Power generators are rebuilt at each tier, not upgraded.

        // Factories: T1 -> T2 -> T3.
        m.insert(
            UnitKind::Factory(TechLevel::T1),
            UnitKind::Factory(TechLevel::T2),
        );
        m.insert(
            UnitKind::Factory(TechLevel::T2),
            UnitKind::Factory(TechLevel::T3),
        );

        m
    }

    /// Hardcoded cap targets for mass extractors.
    ///
    /// Capping is treated as a separate relationship from tier upgrades because
    /// it transforms the same-tier unit into a storage-boosted variant rather
    /// than advancing its tech level.
    fn hardcoded_caps() -> HashMap<UnitKind, UnitKind> {
        let mut m: HashMap<UnitKind, UnitKind> = HashMap::new();

        m.insert(
            UnitKind::Mex(TechLevel::T2),
            UnitKind::CapMex(TechLevel::T2),
        );
        m.insert(
            UnitKind::Mex(TechLevel::T3),
            UnitKind::CapMex(TechLevel::T3),
        );

        m
    }
}

/// True if `builder` and `target` are the same tech tier and `builder` has a
/// lower producer priority than `target`.
///
/// This prevents same-tier bidirectional build edges such as
/// Factory T1 <-> Eng T1: the factory produces the engineer, but the engineer
/// can also rebuild the factory. We keep the producer -> product direction and
/// drop the reverse in the symbolic graph.
fn same_tier_lower_priority_builder(builder: &UnitKind, target: &UnitKind) -> bool {
    match (tech_level_of(builder), tech_level_of(target)) {
        (Some(bt), Some(tt)) if bt == tt => {
            producer_priority(role_of(builder)) < producer_priority(role_of(target))
        }
        _ => false,
    }
}

/// Producer priority for breaking same-tier bidirectional build edges.
///
/// Higher values represent units that are more naturally "producers" in the
/// build order: Commander produces factories, factories produce engineers, and
/// engineers produce structures/upgrades.
fn producer_priority(role: UnitRole) -> i32 {
    match role {
        UnitRole::Commander => 4,
        UnitRole::Factory => 3,
        UnitRole::Engineer => 2,
        UnitRole::EnergyStorage | UnitRole::MassExtractor | UnitRole::PowerGenerator => 1,
        UnitRole::Experimental | UnitRole::Other => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_library() -> FAFBlueprint {
        FAFBlueprint::default().expect("default units should load")
    }

    #[test]
    fn units_answers_build_and_upgrade_questions() {
        let units = load_library();

        // Build questions.
        assert!(units.can_build(&UnitKind::Commander, &UnitKind::Pgen(TechLevel::T1)));
        assert!(units.can_build(
            &UnitKind::Engineer(TechLevel::T1),
            &UnitKind::Pgen(TechLevel::T1)
        ));
        assert!(!units.can_build(&UnitKind::Commander, &UnitKind::Engineer(TechLevel::T3)));

        // Upgrade questions.
        assert!(units.is_upgradeable(&UnitKind::Mex(TechLevel::T1)));
        assert_eq!(
            units.upgrade_target(&UnitKind::Mex(TechLevel::T1)),
            Some(UnitKind::Mex(TechLevel::T2))
        );

        // Non-upgradeable unit.
        assert!(!units.is_upgradeable(&UnitKind::Engineer(TechLevel::T1)));
        assert!(units
            .upgrade_target(&UnitKind::Engineer(TechLevel::T1))
            .is_none());
    }

    #[test]
    fn commander_builds_t1_economy_and_factories() {
        let units = load_library();

        let buildable = units.buildable_by(&UnitKind::Commander);
        assert!(buildable.contains(&UnitKind::Mex(TechLevel::T1)));
        assert!(buildable.contains(&UnitKind::Pgen(TechLevel::T1)));
        assert!(buildable.contains(&UnitKind::Factory(TechLevel::T1)));
        assert!(!buildable.contains(&UnitKind::Engineer(TechLevel::T1)));
    }

    #[test]
    fn factory_builds_its_tier_engineers() {
        let units = load_library();

        let t1 = units.buildable_by(&UnitKind::Factory(TechLevel::T1));
        assert!(t1.contains(&UnitKind::Engineer(TechLevel::T1)));
        assert!(!t1.contains(&UnitKind::Engineer(TechLevel::T2)));

        let t2 = units.buildable_by(&UnitKind::Factory(TechLevel::T2));
        assert!(t2.contains(&UnitKind::Engineer(TechLevel::T2)));
    }

    #[test]
    fn categories_group_common_units() {
        use super::super::types::{category_of, UnitCategory};

        assert_eq!(category_of(&UnitKind::Commander), UnitCategory::Commander);
        assert_eq!(
            category_of(&UnitKind::Engineer(TechLevel::T1)),
            UnitCategory::Engineer
        );
        assert_eq!(
            category_of(&UnitKind::Factory(TechLevel::T1)),
            UnitCategory::Factory
        );
        assert_eq!(
            category_of(&UnitKind::Mex(TechLevel::T1)),
            UnitCategory::Economic
        );
        assert_eq!(
            category_of(&UnitKind::Pgen(TechLevel::T1)),
            UnitCategory::Economic
        );
    }

    #[test]
    fn storage_and_capped_mex_units_are_defined() {
        let units = load_library();

        // Base mass-extractor outputs (these are the canonical FAF values).
        assert!(
            (units.production_per_second_mass(&UnitKind::Mex(TechLevel::T2)) - 6.0).abs() < 1e-9
        );
        assert!(
            (units.production_per_second_mass(&UnitKind::Mex(TechLevel::T3)) - 18.0).abs() < 1e-9
        );

        assert!(units.mass_storage(&UnitKind::CapMex(TechLevel::T2)) > 0.0);
        // A capped mex produces 1.5x the base output (4 storages * +12.5%).
        assert!(
            (units.production_per_second_mass(&UnitKind::CapMex(TechLevel::T2))
                - 1.5 * units.production_per_second_mass(&UnitKind::Mex(TechLevel::T2)))
            .abs()
                < 1e-9
        );
        assert!(
            (units.production_per_second_mass(&UnitKind::CapMex(TechLevel::T2)) - 9.0).abs() < 1e-9
        );

        assert!(units.mass_storage(&UnitKind::CapMex(TechLevel::T3)) > 0.0);
        assert!(
            (units.production_per_second_mass(&UnitKind::CapMex(TechLevel::T3))
                - 1.5 * units.production_per_second_mass(&UnitKind::Mex(TechLevel::T3)))
            .abs()
                < 1e-9
        );
        assert!(
            (units.production_per_second_mass(&UnitKind::CapMex(TechLevel::T3)) - 27.0).abs()
                < 1e-9
        );

        assert!(units.energy_storage(&UnitKind::EnergyStorage) > 0.0);

        assert!(units.can_build(&UnitKind::Engineer(TechLevel::T1), &UnitKind::EnergyStorage));
        assert!(units.is_upgradeable(&UnitKind::Mex(TechLevel::T2)));
        assert!(units.is_cappable(&UnitKind::Mex(TechLevel::T2)));
        assert_eq!(
            units.upgrade_target(&UnitKind::Mex(TechLevel::T2)),
            Some(UnitKind::Mex(TechLevel::T3))
        );
        assert_eq!(
            units.cap_target(&UnitKind::Mex(TechLevel::T2)),
            Some(UnitKind::CapMex(TechLevel::T2))
        );
        assert_eq!(
            units.upgrade_target(&UnitKind::CapMex(TechLevel::T2)),
            Some(UnitKind::CapMex(TechLevel::T3))
        );
    }

    #[test]
    fn build_graph_contains_mex_upgrade_chain() {
        let units = load_library();
        let graph = units.build_graph();

        assert!(graph.node(&UnitKind::Mex(TechLevel::T1)).is_some());
        assert!(graph.node(&UnitKind::Mex(TechLevel::T2)).is_some());
        assert!(graph.node(&UnitKind::Mex(TechLevel::T3)).is_some());
        assert!(graph.node(&UnitKind::CapMex(TechLevel::T3)).is_some());

        let upgrade_targets = |from: &UnitKind| {
            graph
                .upgrades_from(from)
                .map(|(target_idx, _)| graph.graph[target_idx].kind.clone())
                .collect::<Vec<_>>()
        };

        assert!(
            upgrade_targets(&UnitKind::Mex(TechLevel::T1)).contains(&UnitKind::Mex(TechLevel::T2))
        );
        assert!(
            upgrade_targets(&UnitKind::Mex(TechLevel::T2)).contains(&UnitKind::Mex(TechLevel::T3))
        );
        assert!(upgrade_targets(&UnitKind::CapMex(TechLevel::T2))
            .contains(&UnitKind::CapMex(TechLevel::T3)));

        let cap_targets = |from: &UnitKind| {
            graph
                .caps_from(from)
                .map(|(target_idx, _)| graph.graph[target_idx].kind.clone())
                .collect::<Vec<_>>()
        };
        assert!(
            cap_targets(&UnitKind::Mex(TechLevel::T2)).contains(&UnitKind::CapMex(TechLevel::T2))
        );
        assert!(
            cap_targets(&UnitKind::Mex(TechLevel::T3)).contains(&UnitKind::CapMex(TechLevel::T3))
        );

        // Every node should carry role/category metadata.
        for node in graph.graph.node_weights() {
            assert!(!node.display_name.is_empty());
        }
    }

    #[test]
    fn build_graph_includes_factory_and_engineer_build_edges() {
        let units = load_library();
        let graph = units.build_graph();

        let builders_for = |target: &UnitKind| {
            graph
                .builds_for(target)
                .map(|(builder_idx, _)| graph.graph[builder_idx].kind.clone())
                .collect::<Vec<_>>()
        };

        // The ACU can build the T1 factory.
        assert!(builders_for(&UnitKind::Factory(TechLevel::T1)).contains(&UnitKind::Commander));

        // The T1 factory produces the T1 engineer (producer -> product), while
        // the reverse engineer -> same-tier factory edge is dropped to keep the
        // symbolic graph a clean DAG.
        assert!(builders_for(&UnitKind::Engineer(TechLevel::T1))
            .contains(&UnitKind::Factory(TechLevel::T1)));

        let t1_factory_builders = builders_for(&UnitKind::Factory(TechLevel::T1));
        assert!(!t1_factory_builders.contains(&UnitKind::Engineer(TechLevel::T1)));

        let t1_eng_prereq = graph
            .builds_for(&UnitKind::Engineer(TechLevel::T1))
            .find_map(|(_, edge)| match edge {
                BlueprintEdge::BuiltBy { prereq } => prereq.clone(),
                _ => None,
            });
        assert_eq!(t1_eng_prereq, Some(UnitKind::Factory(TechLevel::T1)));
    }
}
