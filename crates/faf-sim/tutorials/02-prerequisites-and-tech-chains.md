# 02 — Prerequisites and Tech Chains

To build an expensive unit you usually need the right builder. That builder may
itself need to be built. This document explains how `faf-sim` represents those
prerequisites and how to read the dependency graph for a target like the
Monkeylord.

---

## 1. Builder categories

Units can only be built by specific builder categories. The game encodes this
with `BUILTBY*` categories on the unit being built:

| Category | Builder required |
|---|---|
| `BUILTBYCOMMANDER` | Any ACU |
| `BUILTBYTIER1ENGINEER` | T1 engineer |
| `BUILTBYTIER2ENGINEER` | T2 engineer |
| `BUILTBYTIER3ENGINEER` | T3 engineer |
| `BUILTBYTIER1FACTORY` | T1 factory |
| `BUILTBYTIER2FACTORY` | T2 factory |
| `BUILTBYTIER3FACTORY` | T3 factory |

`faf-sim` derives this mapping from categories.

```rust
// crates/faf-sim/src/build_graph.rs ~line 20 — BuilderKind
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
```

## 2. Direct and transitive prerequisites

`BuildGraph` answers two questions:

- `builders_for(target_id)`: who can build this unit directly?
- `all_prerequisites_default(target_id)`: what must exist before that builder can
  exist, recursively, stopping at commanders by default?

```rust
// crates/faf-sim/src/build_graph.rs ~line 141 — builders_for
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
```

The transitive expansion uses a breadth-first search with a stop set to avoid
cycles such as factory ↔ engineer:

```rust
// crates/faf-sim/src/build_graph.rs ~line 187 — all_prerequisites
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
```

## 3. The standard land-tech chain

For most land experimentals the shortest prerequisite chain is:

```text
ACU → T1 land factory → T2 land factory → T3 land factory → T3 engineer → target
```

The CLI encodes this chain by faction:

```rust
// apps/faf-sim-cli/src/main.rs ~line 214 — standard_tech_chain
fn standard_tech_chain<'a>(index: &'a DataIndex, faction: Faction) -> Vec<&'a Unit> {
    let ids = match faction {
        Faction::Uef => ["UEL0001", "UEB0101", "UEB0201", "UEB0301", "UEL0309"],
        Faction::Cybran => ["URL0001", "URB0101", "URB0201", "URB0301", "URL0309"],
        Faction::Aeon => ["UAL0001", "UAB0101", "UAB0201", "UAB0301", "UAL0309"],
        Faction::Seraphim => ["XSL0001", "XSB0101", "XSB0201", "XSB0301", "XSL0309"],
    };

    ids.iter().filter_map(|id| index.find_unit(id)).collect()
}
```

## 4. Example: Monkeylord prerequisites

```bash
$ cargo run --bin faf-sim -- deps -c monkeylord
Target: Cybran Monkeylord (URL0402) — Monkeylord / 猴王 [EXPERIMENTAL]

Direct builders:
  URL0001 — Cybran Armored Command Unit / 赛布兰装甲指挥单元 [COMMAND]
  URL0309 — T3 Engineer / T3工程师 [ENGINEER]

Transitive prerequisites:
  URL0309 — T3 Engineer / T3工程师 [ENGINEER]
  URB0301 — T3 Land Factory HQ / T3陆地工厂总部 [FACTORY]
  URB0201 — T2 Land Factory HQ / T2陆地工厂总部 [FACTORY]
  URB0101 — T1 Land Factory / T1陆地工厂 [FACTORY]
```

Notice that the ACU (`URL0001`) is a direct builder but does **not** appear in the
transitive prerequisites because commanders are the default stopping point.

## 5. Study questions

1. Why does the default prerequisite search stop at commanders?
2. The Monkeylord can be built by either the ACU or a T3 engineer. Which path is
likely to be faster in a real game? Why?
3. What would happen if `all_prerequisites` did not have a `stop_at` set?

## 6. Experiment

Try expanding prerequisites for different targets and factions:

```bash
cargo run --bin faf-sim -- deps -u fatboy
cargo run --bin faf-sim -- deps -a czar
cargo run --bin faf-sim -- deps -s ythotha
```

Next: [03-sequential-baseline.md](./03-sequential-baseline.md)
