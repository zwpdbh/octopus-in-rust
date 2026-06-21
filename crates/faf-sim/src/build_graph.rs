//! Pure unit dependency graph for FAF build orders.
//!
//! This module answers the question: "to build unit X, which builders must I
//! already have?" It derives the graph entirely from unit category strings such
//! as `BUILTBYTIER1FACTORY` and `BUILTBYTIER3ENGINEER`.
//!
//! It intentionally ignores economy, build time, map position, and commander
//! enhancement requirements. Those belong in the simulator / optimizer layers.

use std::collections::{HashMap, HashSet, VecDeque};

use faf_units::{DataIndex, Unit};

/// A category of builder that can produce other units.
///
/// The variants mirror the `BUILTBY*` blueprint categories found in the unit
/// index. Commander tiers are kept as distinct variants because the data
/// exposes them, even though the current index does not contain separate
/// tiered commander units (ACU enhancements are not modeled as units).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuilderKind {
    Commander,
    Tier1Commander,
    Tier2Commander,
    Tier3Commander,
    Tier1Engineer,
    Tier2Engineer,
    Tier3Engineer,
    Tier4Engineer,
    Tier1Factory,
    Tier2Factory,
    Tier3Factory,
    Tier4Factory,
    QuantumGate,
}

impl BuilderKind {
    /// Convert a `BUILTBY*` category string into a builder kind.
    pub fn from_build_category(category: &str) -> Option<Self> {
        match category.to_ascii_uppercase().as_str() {
            "BUILTBYCOMMANDER" => Some(BuilderKind::Commander),
            "BUILTBYTIER1COMMANDER" => Some(BuilderKind::Tier1Commander),
            "BUILTBYTIER2COMMANDER" => Some(BuilderKind::Tier2Commander),
            "BUILTBYTIER3COMMANDER" => Some(BuilderKind::Tier3Commander),
            "BUILTBYTIER1ENGINEER" => Some(BuilderKind::Tier1Engineer),
            "BUILTBYTIER2ENGINEER" => Some(BuilderKind::Tier2Engineer),
            "BUILTBYTIER3ENGINEER" => Some(BuilderKind::Tier3Engineer),
            "BUILTBYTIER4ENGINEER" => Some(BuilderKind::Tier4Engineer),
            "BUILTBYTIER1FACTORY" => Some(BuilderKind::Tier1Factory),
            "BUILTBYTIER2FACTORY" => Some(BuilderKind::Tier2Factory),
            "BUILTBYTIER3FACTORY" => Some(BuilderKind::Tier3Factory),
            "BUILTBYTIER4FACTORY" => Some(BuilderKind::Tier4Factory),
            "BUILTBYQUANTUMGATE" => Some(BuilderKind::QuantumGate),
            _ => None,
        }
    }

    /// True if the given unit satisfies the requirements of this builder kind.
    ///
    /// Note: tiered commander kinds fall back to matching any unit with the
    /// `COMMAND` category because the unit index does not contain separate
    /// tiered commander blueprint ids. This is a deliberate simplification for
    /// the pure unit-dependency layer.
    pub fn matches_unit(&self, unit: &Unit) -> bool {
        match self {
            BuilderKind::Commander
            | BuilderKind::Tier1Commander
            | BuilderKind::Tier2Commander
            | BuilderKind::Tier3Commander => unit.has_category("COMMAND"),
            BuilderKind::Tier1Engineer => {
                unit.has_category("ENGINEER") && unit.has_category("TECH1")
            }
            BuilderKind::Tier2Engineer => {
                unit.has_category("ENGINEER") && unit.has_category("TECH2")
            }
            BuilderKind::Tier3Engineer => {
                unit.has_category("ENGINEER") && unit.has_category("TECH3")
            }
            BuilderKind::Tier4Engineer => {
                unit.has_category("ENGINEER") && unit.has_category("TECH4")
            }
            BuilderKind::Tier1Factory => unit.has_category("FACTORY") && unit.has_category("TECH1"),
            BuilderKind::Tier2Factory => unit.has_category("FACTORY") && unit.has_category("TECH2"),
            BuilderKind::Tier3Factory => unit.has_category("FACTORY") && unit.has_category("TECH3"),
            BuilderKind::Tier4Factory => unit.has_category("FACTORY") && unit.has_category("TECH4"),
            BuilderKind::QuantumGate => unit.has_category("QUANTUMGATE"),
        }
    }
}

/// Pure unit dependency graph derived from a `DataIndex`.
#[derive(Debug, Clone)]
pub struct BuildGraph<'a> {
    index: &'a DataIndex,
    /// Cache of builder kinds for each unit id.
    builder_kinds: HashMap<String, Vec<BuilderKind>>,
}

impl<'a> BuildGraph<'a> {
    /// Build the dependency graph from a unit index.
    pub fn new(index: &'a DataIndex) -> Self {
        let mut builder_kinds = HashMap::with_capacity(index.units.len());
        for unit in &index.units {
            let kinds: Vec<BuilderKind> = unit
                .categories
                .iter()
                .filter_map(|c| BuilderKind::from_build_category(c))
                .collect();
            builder_kinds.insert(unit.id.clone(), kinds);
        }
        Self {
            index,
            builder_kinds,
        }
    }

    /// Look up a unit by blueprint id (case-insensitive).
    pub fn unit(&self, id: &str) -> Option<&Unit> {
        self.index.find_unit(id)
    }

    /// Return the builder kinds required to build a unit.
    pub fn builder_kinds_for(&self, id: &str) -> &[BuilderKind] {
        static EMPTY: &[BuilderKind] = &[];
        self.builder_kinds
            .get(id)
            .map(|v| v.as_slice())
            .unwrap_or(EMPTY)
    }

    /// Return all units that can directly build `target_id`.
    ///
    /// When the target belongs to a faction, only builders of the same faction
    /// are returned. Faction-less targets (e.g. neutral campaign units) match
    /// any faction.
    pub fn builders_for(&self, target_id: &str) -> Vec<&Unit> {
        let target = self.index.find_unit(target_id);
        let target_faction = target.and_then(|u| u.faction());
        let kinds = self.builder_kinds_for(target_id);
        if kinds.is_empty() {
            return Vec::new();
        }
        self.index
            .units
            .iter()
            .filter(|u| {
                let kind_matches = kinds.iter().any(|k| k.matches_unit(u));
                if !kind_matches {
                    return false;
                }
                match target_faction {
                    Some(f) => u.faction().map_or(true, |uf| uf.eq_ignore_ascii_case(f)),
                    None => true,
                }
            })
            .collect()
    }

    /// True if `builder_id` can directly build `target_id`.
    pub fn can_build(&self, builder_id: &str, target_id: &str) -> bool {
        let Some(builder) = self.index.find_unit(builder_id) else {
            return false;
        };
        self.builder_kinds_for(target_id)
            .iter()
            .any(|k| k.matches_unit(builder))
    }

    /// Direct prerequisites for a unit: the set of builder units that must
    /// exist before the target can be produced.
    pub fn direct_prerequisites(&self, target_id: &str) -> Vec<&Unit> {
        self.builders_for(target_id)
    }

    /// Transitive prerequisites for a unit.
    ///
    /// Starting from `target_id`, repeatedly expand each prerequisite unless
    /// its id is in `stop_at`. This prevents infinite loops around cycles such
    /// as factory ↔ engineer.
    ///
    /// The returned order is roughly breadth-first from the target.
    pub fn all_prerequisites<'b>(
        &self,
        target_id: &str,
        stop_at: &'b [&'b str],
    ) -> Result<Vec<&Unit>, UnknownUnitError> {
        let Some(start) = self.index.find_unit(target_id) else {
            return Err(UnknownUnitError(target_id.to_string()));
        };

        let stop_set: HashSet<String> = stop_at.iter().map(|s| s.to_ascii_uppercase()).collect();
        let mut visited: HashSet<String> = HashSet::new();
        let mut result: Vec<&Unit> = Vec::new();
        let mut queue: VecDeque<&Unit> = VecDeque::new();

        visited.insert(start.id.to_ascii_uppercase());
        queue.push_back(start);

        while let Some(current) = queue.pop_front() {
            for prereq in self.direct_prerequisites(current.id.as_str()) {
                let key = prereq.id.to_ascii_uppercase();
                if visited.contains(&key) || stop_set.contains(&key) {
                    continue;
                }
                visited.insert(key);
                result.push(prereq);
                queue.push_back(prereq);
            }
        }

        Ok(result)
    }

    /// Convenience: transitive prerequisites with commanders as the default
    /// stopping point.
    pub fn all_prerequisites_default(
        &self,
        target_id: &str,
    ) -> Result<Vec<&Unit>, UnknownUnitError> {
        let commanders: Vec<String> = self
            .index
            .units
            .iter()
            .filter(|u| u.has_category("COMMAND"))
            .map(|u| u.id.clone())
            .collect();
        let refs: Vec<&str> = commanders.iter().map(|s| s.as_str()).collect();
        self.all_prerequisites(target_id, &refs)
    }
}

/// Error returned when a blueprint id is not found in the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownUnitError(pub String);

impl std::fmt::Display for UnknownUnitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown unit blueprint id: {}", self.0)
    }
}

impl std::error::Error for UnknownUnitError {}
