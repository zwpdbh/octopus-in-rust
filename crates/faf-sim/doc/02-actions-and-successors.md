# 2. Actions and Successors

MCTS expands a state by applying every legal action. This chapter describes the action space for FAF build orders, how successors are generated, and how to keep the branching factor under control.

## The three action types

The planner uses a single enum for all moves:

```rust
// crates/faf-sim/src/planner/search.rs ~line 18 — SearchAction enum
pub enum SearchAction {
    /// Build a unit with the given builders.
    Build {
        unit_id: String,
        builders: Vec<NodeId>,
    },
    /// Assist an active project with additional builders.
    Assist {
        project_node: NodeId,
        builders: Vec<NodeId>,
    },
    /// Advance time without issuing a command.
    Wait,
}
```

- `Build` starts a new project for a unit, assigning one or more idle builders to it.
- `Assist` adds idle builders to an already-started project.
- `Wait` advances the simulator by one tick. It is the only legal action when no builder is idle.

## Successor generation

The existing `SearchConfig::successors` function produces every `(GraphState, SearchAction)` pair reachable in one step. MCTS calls this function at expansion time.

```rust
// crates/faf-sim/src/planner/search.rs ~line 47 — SearchConfig::successors (signature)
pub fn successors(
    self,
    index: &DataIndex,
    tech_graph: &TechGraph,
    state: &GraphState,
    goal: &Unit,
    goal_chain: &[(Capability, String)],
) -> Vec<(GraphState, SearchAction)> {
    // ...
}
```

The function does the following:

1. Collect idle builders. If none are idle, return only a `Wait` successor.
2. Determine candidate units that could help reach the goal, using the tech graph and the goal's prerequisite chain.
3. For each candidate unit, try starting a project with all idle builders and with the single fastest capable idle builder.
4. For each active project, try assisting it with all idle builders.
5. Always include a `Wait` successor.

## Why the branching factor matters

The successor list can grow quickly. A state with several idle engineers and factories may have tens of legal actions. MCTS can handle large branching factors better than exhaustive search, but only if each expansion is cheap and the value net is accurate enough to guide selection.

There are two ways to keep the tree manageable:

1. **Action pruning.** Remove obviously bad actions before expansion. For example, never build more power generators than a configured cap, never start a unit that already exists or is already under construction, and prefer goal-relevant candidates.
2. **Action masking.** Generate the full legal set, but let a policy network assign near-zero probability to uninteresting actions so MCTS does not waste visits on them.

The current code already does some pruning via `max_mex_count`, `max_pgen_count`, and the candidate filter. A learned policy prior (see [`06-training-pipeline.md`](./06-training-pipeline.md)) can add a second layer of masking.

## Legal move validation

Not every `SearchAction` is valid in every state. The simulator rejects actions that violate constraints:

- A busy builder cannot start a new project.
- A builder cannot build a unit it is not capable of building.
- A builder cannot assist a non-existent project.

The successor generator pre-filters most illegal actions, and the simulator's `start_project`/`assist_project` methods return errors for the rest. MCTS expansion should never crash on an illegal action; it should simply skip it.

## MCTS expansion

When MCTS selects a node for expansion, it asks:

```text
for each (next_state, action) in successors(state):
    create child MCTS node
    attach action as the move that leads to it
```

Each child becomes a leaf that the value network will evaluate on its first visit. After evaluation, the value is backed up along the path to the root. The details are in [`04-mcts-search.md`](./04-mcts-search.md).

## Key design choice: discrete actions, continuous time

Actions are discrete (build this unit, assist that project, wait), but time is continuous. The simulator advances by a fixed `dt` each tick. MCTS therefore operates on a discretized decision grid. The choice of `dt` matters:

- A small `dt` gives finer control but expands more nodes.
- A large `dt` is faster but may miss tight timing windows.

The existing `PlannerConfig::dt` default is `10.0` seconds for beam search. MCTS can often use a smaller effective `dt` because it focuses computation only on promising branches.
