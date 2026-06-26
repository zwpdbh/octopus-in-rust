# 1. The State Graph

In MCTS, every node in the tree is a **state**. For FAF build-order optimization, the state is the simulator's `GraphState`. This chapter explains the structure of that state and what matters when MCTS evaluates it.

For the full formal model — exact definitions, constraints, and assumptions — see [`model.md`](./model.md). This chapter focuses on the MCTS-relevant view.

## From ACU to goal

The simulator starts with a single completed unit: the ACU (Armored Command Unit). Every other unit is built by assigning builders from the existing graph. The state therefore records:

- every unit that exists,
- when each unit started and finished,
- which builders contributed to which unit,
- the current economy,
- active construction projects.

```rust
// crates/faf-sim/src/sim.rs ~line 205 — GraphState (abbreviated)
// pub struct GraphState {
//     pub time: f64,
//     pub graph: BuildGraph,
//     pub economy: EconomyState,
//     pub active_projects: Vec<OngoingBuild>,
//     pub events: Vec<BuildEvent>,
// }
```

MCTS does not need to invent this representation; it reuses the existing simulator state as its node payload.

## Nodes and edges

Each node in the build graph represents one built unit. It stores at least:

- `unit_id`: the blueprint identifier, e.g., `URL0001` (ACU), `URB0101` (T1 land factory), `URL0402` (Monkeylord).
- `start_time`: when construction began.
- `finish_time`: when construction completed.

A directed edge `A -> B` means unit `A` contributed build power to create unit `B`. Multiple incoming edges mean multiple builders assisted.

The initial graph contains exactly one node: the ACU, with `start_time = finish_time = 0`.

## Builder constraints that shape the tree

Builder behavior creates most of the structure that MCTS must reason about:

- A builder works on **exactly one target at a time**. Its build power is indivisible.
- A builder can contribute to many units over its lifetime, but the construction intervals must not overlap.
- To **start** a project, at least one assigned builder must be able to build the target according to the tech graph.
- Other builders may **assist** a project even if they cannot build the target themselves, provided they are real builders (commanders, engineers, factories).
- For every edge `A -> B`: `finish_time(A) <= start_time(B)`.

These rules determine which `SearchAction` expansions are legal from a given state.

## Economy and stall

The state also contains the economy:

- mass income per second,
- energy income per second,
- mass storage and cap,
- energy storage and cap.

When a resource-producing unit finishes, its production is added to the economy state immediately at its `finish_time`.

If available mass or energy cannot sustain the assigned build power, the effective build rate is reduced proportionally to the most-constrained resource. Energy stall is especially punishing because it slows both construction and mass income. A good MCTS search will learn to avoid it.

For the current model:

- Energy-dependent systems (shields, radar, stealth) are ignored.
- Reclaim is not modeled.

## What MCTS sees

From MCTS's point of view, a state is a snapshot it can evaluate and expand:

```text
GraphState
├── time
├── graph of completed and under-construction units
├── economy
└── active_projects
```

The value network receives a featurized version of this snapshot (see [`03-value-network.md`](./03-value-network.md)). The search loop uses the legal successors of this snapshot to grow the tree (see [`02-actions-and-successors.md`](./02-actions-and-successors.md)).

## Objectives

1. **Primary:** minimize the completion time of the goal unit.
2. **Secondary:** among plans with the same primary time, maximize mass income per mass invested in economy up to that completion time. This prevents the optimizer from rewarding extra economy built after the goal is already reached.

## Assumptions

- The static tech graph is known and fixed.
- Build power, production, and drain are known per blueprint.
- The economy evolves deterministically given a schedule.
- A builder's build power is indivisible across concurrent targets.
- There is exactly one goal unit.
