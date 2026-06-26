//! Symbolic capability-level dependency graph for FAF build orders.
//!
//! Unlike `BuildGraph`, which reasons about concrete blueprint ids, `TechGraph`
//! reasons about abstract capabilities such as `T1Factory`, `T3Engineer`, or
//! `T3ACU`. Edges encode "building this unit requires that capability" and
//! "this unit provides that capability".
//!
//! The graph is bipartite:
//!
//! ```text
//! Capability(ACU) -> Unit(URB0101) -> Capability(T1Factory)
//!     -> Unit(URB0201) -> Capability(T2Factory) -> ...
//! ```
//!
//! This lets a symbolic planner ask questions like "what is the shortest tech
//! chain from the starting ACU to the Monkeylord?" without hard-coding
//! blueprint ids.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::Arc;

use faf_units::{DataIndex, Unit};
use petgraph::graph::{DiGraph, NodeIndex};

/// An abstract capability or builder tier.
///
/// These are the "simple nodes" the user-level planner thinks in: not
/// `URB0301`, but `T3Factory`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Capability {
    /// Starting Armored Command Unit (can build T1 things).
    ACU,
    /// ACU with T2 engineering upgrade.
    T2ACU,
    /// ACU with T3 engineering upgrade.
    T3ACU,
    /// Any T1 factory.
    T1Factory,
    /// Any T2 factory.
    T2Factory,
    /// Any T3 factory.
    T3Factory,
    /// Any T1 engineer.
    T1Engineer,
    /// Any T2 engineer.
    T2Engineer,
    /// Any T3 engineer.
    T3Engineer,
    /// Any quantum gateway.
    QuantumGate,
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Capability::ACU => "ACU",
            Capability::T2ACU => "T2ACU",
            Capability::T3ACU => "T3ACU",
            Capability::T1Factory => "T1Factory",
            Capability::T2Factory => "T2Factory",
            Capability::T3Factory => "T3Factory",
            Capability::T1Engineer => "T1Engineer",
            Capability::T2Engineer => "T2Engineer",
            Capability::T3Engineer => "T3Engineer",
            Capability::QuantumGate => "QuantumGate",
        };
        write!(f, "{}", s)
    }
}

/// A node in the capability graph.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TechNode {
    /// An abstract capability.
    Capability(Capability),
    /// A concrete unit blueprint id.
    Unit(String),
}

/// A capability-level dependency graph.
#[derive(Debug, Clone)]
pub struct TechGraph {
    graph: DiGraph<TechNode, ()>,
    node_map: HashMap<TechNode, NodeIndex>,
    index: Arc<DataIndex>,
}

impl TechGraph {
    /// Build a capability graph from a unit index.
    pub fn new(index: Arc<DataIndex>) -> Self {
        let mut graph = DiGraph::new();
        let mut node_map = HashMap::new();

        let ensure_node = |graph: &mut DiGraph<TechNode, ()>,
                           node_map: &mut HashMap<TechNode, NodeIndex>,
                           node: TechNode| {
            *node_map
                .entry(node.clone())
                .or_insert_with(|| graph.add_node(node))
        };

        for unit in &index.units {
            let unit_node = TechNode::Unit(unit.id.clone());
            let unit_idx = ensure_node(&mut graph, &mut node_map, unit_node);

            // Capability(required) -> Unit(unit)
            for cap in required_capabilities(unit) {
                let cap_idx = ensure_node(&mut graph, &mut node_map, TechNode::Capability(cap));
                graph.add_edge(cap_idx, unit_idx, ());
            }

            // Unit(unit) -> Capability(provided)
            if let Some(cap) = provided_capability(unit) {
                let cap_idx = ensure_node(&mut graph, &mut node_map, TechNode::Capability(cap));
                graph.add_edge(unit_idx, cap_idx, ());
            }
        }

        Self {
            graph,
            node_map,
            index,
        }
    }

    /// Look up the node index for a capability.
    fn capability_index(&self, cap: Capability) -> Option<NodeIndex> {
        self.node_map.get(&TechNode::Capability(cap)).copied()
    }

    /// Look up the node index for a unit.
    fn unit_index(&self, unit_id: &str) -> Option<NodeIndex> {
        self.node_map
            .get(&TechNode::Unit(unit_id.to_string()))
            .copied()
    }

    /// Access the underlying unit index.
    pub fn index(&self) -> &DataIndex {
        &self.index
    }

    /// Return every capability reachable as a prerequisite of `goal_unit_id`,
    /// stopping expansion at `start`.
    ///
    /// This is the *union* of all alternative requirements; a concrete plan
    /// will pick one path via [`Self::prerequisite_chain`]. Factory/engineer
    /// cycles are broken by treating `start` as already satisfied.
    pub fn prerequisites(
        &self,
        goal_unit_id: &str,
        start: Capability,
    ) -> Result<Vec<Capability>, TechGraphError> {
        let goal_idx = self
            .unit_index(goal_unit_id)
            .ok_or_else(|| TechGraphError::UnknownUnit(goal_unit_id.to_string()))?;
        let start_idx = self
            .capability_index(start)
            .ok_or_else(|| TechGraphError::UnknownCapability(start))?;

        let mut caps = HashSet::new();
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(goal_idx);

        while let Some(node) = queue.pop_front() {
            if !visited.insert(node) {
                continue;
            }
            for pred in self
                .graph
                .neighbors_directed(node, petgraph::Direction::Incoming)
            {
                if pred == start_idx {
                    // `start` is already available; don't expand further.
                    if let TechNode::Capability(cap) = &self.graph[pred] {
                        caps.insert(*cap);
                    }
                    continue;
                }
                if let TechNode::Capability(cap) = &self.graph[pred] {
                    caps.insert(*cap);
                }
                queue.push_back(pred);
            }
        }

        Ok(caps.into_iter().collect())
    }

    /// Return a concrete build chain from `start` capability to `goal_unit_id`.
    ///
    /// The returned vector alternates prerequisites and the units that provide
    /// the next capability, ending with the goal unit:
    ///
    /// ```text
    /// [(ACU, "URB0101"), (T1Factory, "URB0201"), (T2Factory, "URB0301"),
    ///  (T3Factory, "URL0309"), (T3Engineer, "URL0402")]
    /// ```
    ///
    /// The unit ids are chosen from units in the same faction as `goal_unit_id`.
    pub fn prerequisite_chain(
        &self,
        goal_unit_id: &str,
        start: Capability,
    ) -> Result<Vec<(Capability, String)>, TechGraphError> {
        let goal_unit = self
            .unit_index(goal_unit_id)
            .ok_or_else(|| TechGraphError::UnknownUnit(goal_unit_id.to_string()))?;
        let start_idx = self
            .capability_index(start)
            .ok_or_else(|| TechGraphError::UnknownCapability(start))?;

        // Determine the goal faction so we only traverse same-faction units.
        let goal_faction = self.graph[goal_unit]
            .as_unit()
            .and_then(|id| faction_of(id));

        // Determine the preferred factory domain from the goal unit (LAND, AIR,
        // NAVAL) so that, for example, a land experimental uses land factories.
        let factory_domain = self
            .index
            .find_unit(goal_unit_id)
            .and_then(unit_factory_domain);

        // BFS over the bipartite graph, restricted to the goal faction.
        let mut dist: HashMap<NodeIndex, usize> = HashMap::new();
        let mut prev: HashMap<NodeIndex, NodeIndex> = HashMap::new();
        let mut queue = VecDeque::new();

        dist.insert(start_idx, 0);
        queue.push_back(start_idx);

        while let Some(current) = queue.pop_front() {
            if current == goal_unit {
                break;
            }
            let d = dist[&current];
            for neighbor in self.graph.neighbors(current) {
                if dist.contains_key(&neighbor) {
                    continue;
                }
                // When moving from a capability to a unit, stay in the goal faction
                // and respect the factory domain preference.
                if let TechNode::Unit(id) = &self.graph[neighbor] {
                    if let Some(f) = goal_faction {
                        if faction_of(id).map_or(true, |uf| !uf.eq_ignore_ascii_case(f)) {
                            continue;
                        }
                    }
                    if let Some(domain) = factory_domain {
                        let unit = self.index.find_unit(id);
                        if let Some(cap) = unit.and_then(provided_capability) {
                            if is_factory_capability(cap) {
                                let matches_domain = unit.map_or(false, |u| u.has_category(domain));
                                if !matches_domain {
                                    continue;
                                }
                            }
                        }
                    }
                }
                dist.insert(neighbor, d + 1);
                prev.insert(neighbor, current);
                queue.push_back(neighbor);
            }
        }

        if !prev.contains_key(&goal_unit) && goal_unit != start_idx {
            return Err(TechGraphError::NotReachable {
                goal: goal_unit_id.to_string(),
                start,
            });
        }

        // Reconstruct the path from goal back to start.
        let mut path = Vec::new();
        let mut current = goal_unit;
        while let Some(&p) = prev.get(&current) {
            path.push(current);
            current = p;
        }
        path.push(start_idx);
        path.reverse();

        // Convert the alternating Capability -> Unit path into pairs.
        let mut chain = Vec::new();
        for window in path.windows(2) {
            let cap_idx = window[0];
            let unit_idx = window[1];
            let cap = match &self.graph[cap_idx] {
                TechNode::Capability(c) => *c,
                TechNode::Unit(_) => continue,
            };
            let unit_id = match &self.graph[unit_idx] {
                TechNode::Unit(id) => id.clone(),
                TechNode::Capability(_) => continue,
            };
            chain.push((cap, unit_id));
        }

        Ok(chain)
    }

    /// True if `owned` contains a unit that satisfies `cap`.
    pub fn has_capability(owned: &[&Unit], cap: Capability) -> bool {
        owned.iter().any(|u| unit_provides_capability(u, cap))
    }

    /// Return the concrete units that can directly build `target_id`.
    ///
    /// When `target_id` belongs to a faction, only builders of the same faction
    /// are returned. Faction-less targets match any faction.
    pub fn builders_for(&self, target_id: &str) -> Result<Vec<&Unit>, TechGraphError> {
        let target = self
            .index
            .find_unit(target_id)
            .ok_or_else(|| TechGraphError::UnknownUnit(target_id.to_string()))?;
        let required = required_capabilities(target);
        let target_faction = target.faction();

        Ok(self
            .index
            .units
            .iter()
            .filter(|u| {
                if let Some(f) = target_faction {
                    if u.faction().map_or(true, |uf| !uf.eq_ignore_ascii_case(f)) {
                        return false;
                    }
                }
                if let Some(provided) = provided_capability(u) {
                    required.contains(&provided)
                } else {
                    false
                }
            })
            .collect())
    }

    /// Direct prerequisites for a unit: the builders that must exist to produce
    /// it.
    pub fn direct_prerequisites(&self, target_id: &str) -> Result<Vec<&Unit>, TechGraphError> {
        self.builders_for(target_id)
    }

    /// True if `builder_id` can directly build `target_id`.
    pub fn can_build(&self, builder_id: &str, target_id: &str) -> Result<bool, TechGraphError> {
        let builders = self.builders_for(target_id)?;
        Ok(builders
            .iter()
            .any(|u| u.id.eq_ignore_ascii_case(builder_id)))
    }

    /// Transitive prerequisites for a unit.
    ///
    /// Starting from `target_id`, repeatedly expand each prerequisite unless
    /// its id is in `stop_at`. This prevents infinite loops around cycles such
    /// as factory ↔ engineer.
    pub fn all_prerequisites<'b>(
        &self,
        target_id: &str,
        stop_at: &'b [&'b str],
    ) -> Result<Vec<&Unit>, TechGraphError> {
        let start = self
            .index
            .find_unit(target_id)
            .ok_or_else(|| TechGraphError::UnknownUnit(target_id.to_string()))?;

        let stop_set: HashSet<String> = stop_at.iter().map(|s| s.to_ascii_uppercase()).collect();
        let mut visited: HashSet<String> = HashSet::new();
        let mut result: Vec<&Unit> = Vec::new();
        let mut queue: VecDeque<&Unit> = VecDeque::new();

        visited.insert(start.id.to_ascii_uppercase());
        queue.push_back(start);

        while let Some(current) = queue.pop_front() {
            for prereq in self.direct_prerequisites(current.id.as_str())? {
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
    pub fn all_prerequisites_default(&self, target_id: &str) -> Result<Vec<&Unit>, TechGraphError> {
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

/// Determine which capabilities are required to build `unit`.
fn required_capabilities(unit: &Unit) -> Vec<Capability> {
    unit.categories
        .iter()
        .filter_map(|c| builder_category_to_capability(c))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}

/// Map a `BUILTBY*` category to the capability it requires.
fn builder_category_to_capability(category: &str) -> Option<Capability> {
    match category.to_ascii_uppercase().as_str() {
        "BUILTBYCOMMANDER" | "BUILTBYTIER1COMMANDER" => Some(Capability::ACU),
        "BUILTBYTIER2COMMANDER" => Some(Capability::T2ACU),
        "BUILTBYTIER3COMMANDER" => Some(Capability::T3ACU),
        "BUILTBYTIER1ENGINEER" => Some(Capability::T1Engineer),
        "BUILTBYTIER2ENGINEER" => Some(Capability::T2Engineer),
        "BUILTBYTIER3ENGINEER" => Some(Capability::T3Engineer),
        "BUILTBYTIER4ENGINEER" => Some(Capability::T3Engineer),
        "BUILTBYTIER1FACTORY" => Some(Capability::T1Factory),
        "BUILTBYTIER2FACTORY" => Some(Capability::T2Factory),
        "BUILTBYTIER3FACTORY" => Some(Capability::T3Factory),
        "BUILTBYTIER4FACTORY" => Some(Capability::T3Factory),
        "BUILTBYQUANTUMGATE" => Some(Capability::QuantumGate),
        _ => None,
    }
}

/// Determine which capability `unit` provides once built.
fn provided_capability(unit: &Unit) -> Option<Capability> {
    if unit.has_category("COMMAND") {
        return Some(Capability::ACU);
    }
    if unit.has_category("ENGINEER") {
        if unit.has_category("TECH1") {
            return Some(Capability::T1Engineer);
        }
        if unit.has_category("TECH2") {
            return Some(Capability::T2Engineer);
        }
        if unit.has_category("TECH3") {
            return Some(Capability::T3Engineer);
        }
    }
    if unit.has_category("FACTORY") {
        if unit.has_category("TECH1") {
            return Some(Capability::T1Factory);
        }
        if unit.has_category("TECH2") {
            return Some(Capability::T2Factory);
        }
        if unit.has_category("TECH3") {
            return Some(Capability::T3Factory);
        }
    }
    if unit.has_category("QUANTUMGATE") {
        return Some(Capability::QuantumGate);
    }
    None
}

/// True if `unit` satisfies `cap`.
fn unit_provides_capability(unit: &Unit, cap: Capability) -> bool {
    provided_capability(unit) == Some(cap)
}

/// True if `cap` is a factory capability whose units have LAND/AIR/NAVAL
/// variants.
fn is_factory_capability(cap: Capability) -> bool {
    matches!(
        cap,
        Capability::T1Factory | Capability::T2Factory | Capability::T3Factory
    )
}

/// Determine the factory domain (LAND, AIR, NAVAL) implied by a unit's categories.
fn unit_factory_domain(unit: &Unit) -> Option<&'static str> {
    if unit.has_category("LAND") {
        Some("LAND")
    } else if unit.has_category("AIR") {
        Some("AIR")
    } else if unit.has_category("NAVAL") {
        Some("NAVAL")
    } else {
        None
    }
}

/// Extract the two-letter faction prefix from a blueprint id, if any.
fn faction_of(id: &str) -> Option<&str> {
    if id.len() >= 2 {
        Some(&id[..2])
    } else {
        None
    }
}

impl TechNode {
    fn as_unit(&self) -> Option<&str> {
        match self {
            TechNode::Unit(id) => Some(id),
            _ => None,
        }
    }
}

/// Errors that can occur when querying a `TechGraph`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TechGraphError {
    UnknownUnit(String),
    UnknownCapability(Capability),
    NotReachable { goal: String, start: Capability },
}

impl fmt::Display for TechGraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TechGraphError::UnknownUnit(id) => write!(f, "unknown unit blueprint id: {}", id),
            TechGraphError::UnknownCapability(cap) => {
                write!(f, "unknown capability node: {}", cap)
            }
            TechGraphError::NotReachable { goal, start } => write!(
                f,
                "goal {} is not reachable from capability {}",
                goal, start
            ),
        }
    }
}

impl std::error::Error for TechGraphError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_index() -> std::sync::Arc<DataIndex> {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        std::sync::Arc::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn monkeylord_chain_via_land_tech() {
        let index = load_index();
        let graph = TechGraph::new(index);

        let chain = graph
            .prerequisite_chain("URL0402", Capability::ACU)
            .expect("Monkeylord reachable from ACU");

        let caps: Vec<Capability> = chain.iter().map(|(c, _)| *c).collect();
        assert_eq!(
            caps,
            vec![
                Capability::ACU,
                Capability::T1Factory,
                Capability::T2Factory,
                Capability::T3Factory,
                Capability::T3Engineer,
            ]
        );

        // Each step should use a Cybran unit.
        for (_, id) in &chain {
            assert!(
                id.starts_with("UR") || id.starts_with("XR"),
                "expected Cybran unit, got {}",
                id
            );
        }

        // Last step builds the goal itself.
        assert_eq!(chain.last().map(|(_, id)| id.as_str()), Some("URL0402"));
    }

    #[test]
    fn fatboy_chain_via_land_tech() {
        let index = load_index();
        let graph = TechGraph::new(index);

        let chain = graph
            .prerequisite_chain("UEL0401", Capability::ACU)
            .expect("Fatboy reachable from ACU");

        let caps: Vec<Capability> = chain.iter().map(|(c, _)| *c).collect();
        assert_eq!(
            caps,
            vec![
                Capability::ACU,
                Capability::T1Factory,
                Capability::T2Factory,
                Capability::T3Factory,
                Capability::T3Engineer,
            ]
        );

        for (_, id) in &chain {
            assert!(
                id.starts_with("UE") || id.starts_with("XE"),
                "expected UEF unit, got {}",
                id
            );
        }
    }

    #[test]
    fn t1_mex_only_needs_acu_or_t1_engineer() {
        let index = load_index();
        let graph = TechGraph::new(index);

        let chain = graph
            .prerequisite_chain("URB1103", Capability::ACU)
            .expect("T1 mex reachable from ACU");

        assert_eq!(
            chain,
            vec![(Capability::ACU, "URB1103".to_string())],
            "base ACU should build T1 mex directly"
        );

        let prereqs = graph
            .prerequisites("URB1103", Capability::ACU)
            .expect("valid goal");
        assert!(prereqs.contains(&Capability::ACU));
        assert!(prereqs.contains(&Capability::T1Engineer));
    }

    #[test]
    fn t3_pgen_needs_t3_builder() {
        let index = load_index();
        let graph = TechGraph::new(index);

        let chain = graph
            .prerequisite_chain("URB1301", Capability::ACU)
            .expect("T3 pgen reachable from ACU");

        let caps: Vec<Capability> = chain.iter().map(|(c, _)| *c).collect();
        assert_eq!(
            caps,
            vec![
                Capability::ACU,
                Capability::T1Factory,
                Capability::T2Factory,
                Capability::T3Factory,
                Capability::T3Engineer,
            ]
        );
    }
}
