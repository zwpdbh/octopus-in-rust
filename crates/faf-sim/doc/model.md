# Build-Order Optimization Model

## Core idea

A FAF build order is a directed graph that grows over time. Nodes are built units; edges record which existing units contributed build power to create a new unit. The optimization problem is to grow this graph from the starting ACU until it contains the goal unit, while minimizing the time when the goal unit finishes.

## Definitions

### Nodes

- Each node represents one built-unit slot. A slot can be constructed from scratch or created by upgrading an earlier unit, which retires the earlier slot.
- Every node stores:
  - `unit_id`: the current blueprint identifier, e.g., `URL0001` (ACU), `URB0101` (T1 land factory), `URL0402` (Monkeylord).
  - `state`: a lifecycle tag describing whether the slot is under construction or finished, and whether it was reached by construction or by upgrade.
- The initial graph contains exactly one node: the ACU, finished at time 0.

### Node lifecycle

```rust
// crates/faf-sim/src/sim/state.rs ~line 91 — UnitNodeState (abbreviated)
pub enum UnitNodeState {
    Constructing { start: f64, started_by: Vec<NodeId>, assisted_by: Vec<NodeId> },
    Upgrading { start: f64, from_unit_id: UnitKind, started_by: Vec<NodeId>, assisted_by: Vec<NodeId> },
    Constructed { start_time: f64, finish_time: f64 },
    Upgraded { start_time: f64, finish_time: f64, from_unit_id: UnitKind },
    Replaced { start_time: f64, finish_time: f64, into: NodeId },
}
```

The state machine is flat: there are no nested `Building`/`Finished` wrappers. `Constructed` and `Upgraded` are active finished states; `Constructing` and `Upgrading` are in-progress states; `Replaced` is a retired state used for the source node of an upgrade.

An upgrade creates a **new** node and retires the source slot. When an upgrade starts, the source node moves to `Replaced { into: new_node }` so it no longer contributes to the economy or acts as a builder. The new node starts in `Upgrading { from_unit_id: old_kind }` and finishes as `Upgraded { from_unit_id: old_kind }`. This makes the upgrade history explicit in the graph while keeping the active unit set unambiguous.

### Edges

- A directed edge `A -> B` means: unit `A` contributed build power to construct unit `B`.
- Multiple edges `A1 -> B, A2 -> B, ...` represent assistance. The total build power assigned to `B` is the sum of the build power of all source nodes.
- To **start** a new project for `B`, at least one assigned builder must be able to build `unit_type(B)` according to the static tech/capability graph.
- Additional builders may **assist** an already-started project even if they cannot build the target themselves, as long as they are real builders (commanders, engineers, or factories).

### Builder constraints

- A builder node works on **exactly one target at a time**. Its build power cannot be split across multiple concurrent targets.
- A builder node may have **multiple outgoing edges over its lifetime**, each to a target built at a different time. The construction intervals of those targets must not overlap.
- For every edge `A -> B`: `finish_time(A) <= start_time(B)`. (The builder must exist before it can build anything.)

### Graph growth

Building a new unit means:

1. Choose a set of existing builder nodes that can build the desired unit id.
2. Create a new node `B` for the target unit.
3. Add edges from each chosen builder to `B`.
4. Compute `start_time(B)` and `finish_time(B)` from the eco-drain model.

Upgrading an existing unit means:

1. Choose a finished unit slot `A` that has a registered upgrade target.
2. Create a new node `B` for the upgraded unit and set `A` to `Replaced { into: B }`.
3. Set `B`'s state to `Upgrading { from_unit_id: old_id }` and add edges from the builders working on the upgrade to `B`.
4. Compute the upgrade's finish time from the upgrade cost.

Upgrade costs and target mappings are stored in the `UpgradeTable`, which is part of the `Units` repository (see below). The simulator does not read upgrade information from the raw blueprint data.

### Time and economy

- The build duration of a node depends on:
  - its assigned build power (sum of incoming builder build rates),
  - the mass/energy available during its construction interval,
  - whether the economy stalls.
- When a resource-producing node finishes, its production effect is added to the economy state **immediately** at `finish_time(node)`.
- The economy state consists of:
  - mass income per second,
  - energy income per second,
  - mass storage and storage cap,
  - energy storage and energy cap.

### Stall modeling

- If available mass or energy is insufficient to sustain the assigned build power, the effective build rate is **reduced proportionally** to the most-constrained resource.
- When energy storage is empty and net energy income is zero or negative, mass production is scaled down proportionally to the energy shortfall. At full energy availability, mass production is 100%; at zero available energy, mass production drops to 0%.
- Because energy stall reduces both construction speed and mass income, the optimizer should avoid energy stall whenever possible.
- Energy-dependent systems (shields, radar, stealth) are ignored for this model.
- Reclaim is **not modeled**.

## Objectives

1. **Primary**: minimize the completion time of the goal unit.
2. **Secondary**: among plans with the same primary completion time, maximize mass income per mass invested in economy up to the moment the goal unit finishes.

## Unit knowledge repository

All static unit knowledge is accessed through the `Units` abstraction in `crates/faf-sim/src/units/mod.rs`. `Units` owns a copy of the raw `faf-units` index and builds derived structures (`TechGraph`, `UpgradeTable`) from it. The rest of `faf-sim` does not import `faf-units` directly.

```rust
// crates/faf-sim/src/units/mod.rs ~line 39 — Units (abbreviated)
pub struct Units {
    defs: HashMap<UnitKind, UnitDef>,
    builds: HashMap<UnitKind, BuildRecipe>,
    upgrades: HashMap<UnitKind, Vec<UpgradeRecipe>>,
}
```

This keeps the FAF community data format isolated in one place and lets `faf-sim` add game-specific interpretations (upgrade chains, capability graph) without polluting the raw data crate.

## Assumptions

- Unit knowledge is provided by `Units`; the static tech graph and upgrade table are known and fixed for a given `Units` instance.
- Build power, production, and drain are known per unit blueprint.
- The economy evolves deterministically given a schedule.
- A builder's build power is indivisible across concurrent targets.
- There is exactly one goal unit.
- Energy stall reduces mass income linearly with available energy. (This is a working assumption; verify against FAF Lua/source when possible.)

## Notes

- The secondary objective uses mass income per mass invested in economy as an efficiency metric, evaluated only up to the primary completion time. This prevents the optimizer from rewarding extra economy built after the goal is already reached.
