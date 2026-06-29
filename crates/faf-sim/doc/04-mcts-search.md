# 4. MCTS Search

This chapter describes the **planned** UCT (Upper Confidence Bound applied to Trees) search loop and the current one-step macro-direction policy that sits underneath it.

## Current status: one-step macro policy

The `Strategy::Mcts` planner currently runs a **one-step macro-direction policy**, not a full UCT tree search. At each decision tick it:

1. Derives `SelectionPools` from the `PlanGraph` and current `GraphState`.
2. Computes state features and runs them through the learned macro network.
3. Picks the highest-scoring macro direction (greedy) or samples one (stochastic).
4. Uses the deterministic resolver to turn the direction into a concrete, executable candidate.

This is implemented in `mcts::plan`:

```rust
// crates/faf-sim/src/planner/mcts/mod.rs ~line 44 — mcts::plan
pub fn plan(
    units: &Units,
    initial_state: GraphState,
    goal_id: &UnitKind,
    _iterations: usize,
    value_net_kind: ValueNetKind,
    deterministic: bool,
    value_net: Option<MacroNet<TrainBackend>>,
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    match value_net_kind {
        ValueNetKind::Mlp => {
            macro_policy_plan(units, initial_state, goal_id, value_net, deterministic, config)
        }
        ValueNetKind::Gnn => Err(PlannerError::UnsupportedStrategy(
            "GNN value net is not yet implemented".to_string(),
        )),
    }
}
```

The `_iterations` parameter is ignored because there is no tree yet. Full UCT will use the same `MacroNet` and the same resolver; it will add tree search on top.

## Planned UCT design

When UCT is implemented, each MCTS iteration will repeat four steps:

1. **Select.** Traverse from the root to a leaf using the UCT formula.
2. **Expand.** Add one or more children to the leaf using `SelectionPools::new`.
3. **Evaluate.** Run the policy/value network on each new child (or use the terminal outcome if the state is done).
4. **Backup.** Add the evaluated value to every node on the path from the new child back to the root.

### Node structure

Each MCTS node stores the simulator state at that point in the tree plus statistics:

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 23 — MctsNode
pub struct MctsNode {
    /// Simulator state at this node.
    pub state: GraphState,
    /// Total value accumulated from backpropagation.
    pub total_value: f64,
    /// Number of times this node has been visited.
    pub visits: usize,
    /// Child nodes.
    pub children: Vec<MctsNode>,
}
```

### Selection

At each internal node, pick the child that maximizes **UCB1**:

```text
UCB1(child) = (child.total_value / child.visits)
              + c_puct * sqrt(ln(parent.visits) / child.visits)
```

The first term is exploitation: children with high average value are preferred. The second term is exploration: children with few visits get a bonus. `c_puct` controls the balance.

The configuration is captured in `MctsConfig`:

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 14 — MctsConfig
pub struct MctsConfig {
    /// Number of MCTS iterations (selection/expansion/evaluation/backup loops).
    pub iterations: usize,
    /// UCT exploration constant.
    pub c_puct: f64,
}
```

A larger `c_puct` makes the search explore more aggressively. A smaller `c_puct` makes it greedier. The right value depends on the noise in your value estimates; start near `sqrt(2)` and tune on benchmarks.

### Expansion

When the selected node is not fully expanded, generate one of its untried legal candidates:

```text
action = pop untried candidate
next_state = apply(candidate, state)
child = MctsNode { state: next_state, total_value: 0.0, visits: 0, children: [] }
add child to node.children
```

If the node is already fully expanded, selection continues deeper.

### Evaluation

After expansion, evaluate the new child:

- If the state has reached the goal, the value is the exact terminal value.
- Otherwise, featurize the state and run the learned network.

The scaffold currently leaves the search loop unimplemented:

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 56 — MctsSearch::search
pub fn search(
    &self,
    _initial_state: GraphState,
    _goal_id: &UnitKind,
    _units: &Units,
    _planner_config: &PlannerConfig,
    _value_net: &MacroNet<NdArray>,
) -> Result<PlanResult, PlannerError> {
    let _ = self.config;
    todo!("MCTS search loop is not yet implemented")
}
```

### Backup

Add the evaluated value to `total_value` and increment `visits` for every node on the path from the new child to the root. This is what makes UCB1 work: averages are updated, and exploration bonuses shrink as visit counts grow.

## Choosing the final action

After the iteration budget is exhausted, pick the root child with the **highest visit count**, not necessarily the highest average value. Visit count is a more robust signal because it reflects how much search effort was directed at the move.

```rust
// docref: example
let best_child = root.children.iter()
    .max_by_key(|c| c.visits)
    .expect("root has been expanded");
```

## Tree reuse

Because the planner runs every tick, you can reuse parts of the previous tree. After executing the chosen action, the corresponding child becomes the new root; its siblings and their subtrees are discarded. This saves the search effort already invested in the branch you actually follow.

Tree reuse is optional but valuable when you have a tight per-tick time budget. The main complication is that the simulator state must match the stored child state exactly; any drift requires rebuilding from scratch.

## Search budget

Two common budget modes:

- **Iteration budget:** run exactly `N` iterations. Simple and reproducible.
- **Time budget:** run as many iterations as possible within `T` milliseconds. Better for real-time use.

The `Strategy::Mcts` variant exposes the iteration count, the value-net kind, and a deterministic flag:

```rust
// crates/faf-sim/src/planner/core.rs ~line 105 — Strategy::Mcts variant
Mcts {
    /// Number of MCTS iterations to run per decision.
    iterations: usize,
    /// Kind of learned value network to use inside MCTS.
    value_net: ValueNetKind,
    /// If true, always pick the highest-scoring macro direction.
    deterministic: bool,
},
```

You can extend this later with a time budget and a parallel search worker.
