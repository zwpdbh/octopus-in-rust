# 02 — Prerequisites and Tech Chains

To build an expensive unit you usually need the right builder. That builder may
itself need to be built. This document explains how `faf-sim` represents those
prerequisites and how to read the dependency graph for a target like the
Monkeylord.

---

## 1. Builder categories

Units can only be built by specific builder categories. The game encodes this
with `BUILTBY*` categories on the unit being built:

| Category | Capability required |
|---|---|
| `BUILTBYCOMMANDER` | `ACU` |
| `BUILTBYTIER1ENGINEER` | `T1Engineer` |
| `BUILTBYTIER2ENGINEER` | `T2Engineer` |
| `BUILTBYTIER3ENGINEER` | `T3Engineer` |
| `BUILTBYTIER1FACTORY` | `T1Factory` |
| `BUILTBYTIER2FACTORY` | `T2Factory` |
| `BUILTBYTIER3FACTORY` | `T3Factory` |

`faf-sim` collapses these concrete categories into abstract capabilities in
`TechGraph`:

```rust
// crates/faf-sim/src/tech_graph.rs ~line 27 — Capability
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Capability {
    ACU,
    T2ACU,
    T3ACU,
    T1Factory,
    T2Factory,
    T3Factory,
    T1Engineer,
    T2Engineer,
    T3Engineer,
    QuantumGate,
}
```

## 2. Direct and transitive prerequisites

`TechGraph` answers prerequisite questions as a bipartite directed graph:

```text
Capability(required) -> Unit -> Capability(provided)
```

- `builders_for(target_id)`: which concrete units can build this unit directly?
- `all_prerequisites_default(target_id)`: what concrete units must exist before
  that builder can exist, recursively, stopping at commanders by default?
- `prerequisite_chain(target_id, ACU)`: the shortest symbolic tech chain.

```rust
// crates/faf-sim/src/tech_graph.rs ~line 380 — builders_for
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
```

The transitive expansion uses a breadth-first search with a stop set to avoid
cycles such as factory ↔ engineer:

```rust
// crates/faf-sim/src/tech_graph.rs ~line 460 — all_prerequisites
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
```

## 3. The standard land-tech chain

For most land experimentals the shortest prerequisite chain is:

```text
ACU → T1 land factory → T2 land factory → T3 land factory → T3 engineer → target
```

The default chain by faction is:

```rust
// docref: example
let ids = match faction {
    Faction::Uef => ["UEL0001", "UEB0101", "UEB0201", "UEB0301", "UEL0309"],
    Faction::Cybran => ["URL0001", "URB0101", "URB0201", "URB0301", "URL0309"],
    Faction::Aeon => ["UAL0001", "UAB0101", "UAB0201", "UAB0301", "UAL0309"],
    Faction::Seraphim => ["XSL0001", "XSB0101", "XSB0201", "XSB0301", "XSL0309"],
};
```

## 4. Example: Monkeylord prerequisites

```bash
$ cargo run --bin faf-sim -- deps -c monkeylord
Target: Cybran Monkeylord (URL0402) — Monkeylord / 猴王 [EXPERIMENTAL]

Direct builders:
  URL0309 — T3 Engineer / T3工程师 [ENGINEER]

Transitive prerequisites:
  URL0309 — T3 Engineer / T3工程师 [ENGINEER]
  URB0301 — T3 Land Factory HQ / T3陆地工厂总部 [FACTORY]
  URB0201 — T2 Land Factory HQ / T2陆地工厂总部 [FACTORY]
  URB0101 — T1 Land Factory / T1陆地工厂 [FACTORY]
```

In the capability model, the base ACU (`URL0001`) is **not** a direct builder of
the Monkeylord because the Monkeylord requires a `T3Engineer` or `T3ACU`
capability. The ACU only provides the `ACU` capability. This is more accurate
than the old unit-level graph, which simplified all commander tiers into the
same node.

The `plan` subcommand shows the symbolic chain:

```bash
$ cargo run --bin faf-sim -- plan -c monkeylord
Symbolic tech chain:
 1. ACU → build URB0101 (T1 Land Factory)
 2. T1Factory → build URB0201 (T2 Land Factory)
 3. T2Factory → build URB0301 (T3 Land Factory)
 4. T3Factory → build URL0309 (T3 Engineer)
 5. T3Engineer → build Monkeylord (URL0402)
```

## 5. Study questions

1. Why does the default prerequisite search stop at commanders?
2. The Monkeylord can be built by a T3 engineer. What capabilities must you
   acquire before you can build that engineer?
3. What would happen if `all_prerequisites` did not have a `stop_at` set?

## 6. Experiment

Try expanding prerequisites for different targets and factions:

```bash
cargo run --bin faf-sim -- deps -u fatboy
cargo run --bin faf-sim -- deps -a czar
cargo run --bin faf-sim -- deps -s ythotha
```

Next: [03-sequential-baseline.md](./03-sequential-baseline.md)
