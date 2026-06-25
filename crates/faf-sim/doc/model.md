# Build-Order Optimization Model

## Core idea

A FAF build order is a directed graph that grows over time. Nodes are built units; edges record which existing units contributed build power to create a new unit. The optimization problem is to grow this graph from the starting ACU until it contains all goal units, while minimizing the time when the last goal unit finishes.

## Definitions

### Nodes

- Each node represents one built unit.
- Every node stores:
  - `unit_type`: the blueprint/unit kind (e.g., `URL0001`, `URB0101`, `URL0402`).
  - `start_time`: when construction of this unit began.
  - `finish_time`: when construction of this unit completed.
- The initial graph contains exactly one node: the ACU, with `start_time = finish_time = 0`.

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

1. Choose a set of existing builder nodes that can build the desired unit type.
2. Create a new node `B` for the target unit.
3. Add edges from each chosen builder to `B`.
4. Compute `start_time(B)` and `finish_time(B)` from the eco-drain model.

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

1. **Primary**: minimize the completion time of the last goal unit.
2. **Secondary**: among plans with the same primary completion time, maximize mass income per mass invested in economy up to the moment the last goal unit finishes.

## Assumptions

- The static tech graph (who can build whom) is known and fixed.
- Build power, production, and drain are known per unit blueprint.
- The economy evolves deterministically given a schedule.
- A builder's build power is indivisible across concurrent targets.
- There may be one or more goal units.
- Energy stall reduces mass income linearly with available energy. (This is a working assumption; verify against FAF Lua/source when possible.)

## Notes

- The secondary objective uses mass income per mass invested in economy as an efficiency metric, evaluated only up to the primary completion time. This prevents the optimizer from rewarding extra economy built after the goal is already reached.
