# 7. MCTS Search

This chapter describes the UCT (Upper Confidence Bound applied to Trees) search loop that sits on top of the hierarchical policy. The policy provides prior probabilities over plan-graph edges and a default rollout policy; MCTS turns those into a closed-loop planner.

## Search loop

Each MCTS iteration repeats four steps:

1. **Select.** Traverse from the root to a leaf using the UCT formula.
2. **Expand.** Add one or more children to the leaf using the legal plan-graph edges.
3. **Evaluate.** Run the policy/value network on each new child (or use the terminal outcome if the state is done).
4. **Backup.** Add the evaluated value to every node on the path from the new child back to the root.

These four steps are implemented in `MctsSearch::search`:

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 115 — MctsSearch::search
pub fn search(
    &self,
    initial_state: GraphState,
    goal_id: &UnitKind,
    units: &Units,
    planner_config: &PlannerConfig,
    model: &PolicyBundle<TrainBackend>,
) -> Result<PlanResult, PlannerError> {
    let edge_index = PlanEdgeIndex::new(&units.plan_graph(goal_id)?);
    let device: TrainDevice = Default::default();
    let mut root = MctsNode::new(initial_state, goal_id, units, planner_config, &edge_index, model, &device);

    for _ in 0..self.config.iterations {
        let path = select_path(&root, self.config.c_puct);
        // ... walk to leaf, expand or rollout, backup value ...
    }

    // Pick the root child with the highest visit count.
    // ...
}
```

If `iterations` is zero, the planner falls back to the one-step hierarchical policy directly, preserving the behavior of an untrained or deterministic policy.

## Node structure

Each MCTS node stores the simulator state at that point in the tree plus statistics:

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 47 — MctsNode
struct MctsNode {
    state: GraphState,
    total_value: f64,
    visits: usize,
    children: Vec<(usize, Box<MctsNode>)>,
    untried_edges: Vec<usize>,
    edge_priors: Vec<f32>,
    is_terminal: bool,
}
```

- `state` is the simulator state after applying the edge that leads to this node.
- `total_value` is the sum of all backed-up values.
- `visits` is the number of times the node has been visited.
- `children` maps each expanded edge index to its child node.
- `untried_edges` lists legal edges that have not been expanded yet.
- `edge_priors` stores the network's prior probability for each edge.
- `is_terminal` is true if the state has reached the goal.

## Selection

At each internal node, `select_path` picks the child that maximizes the PUCT formula:

```text
PUCT(child) = (child.total_value / child.visits)
              + c_puct * prior * sqrt(parent.visits) / (1.0 + child.visits)
```

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 282 — select_path
fn select_path(node: &MctsNode, c_puct: f64) -> Vec<usize> {
    // ... while not at leaf, pick child with best q + u ...
}
```

The first term is exploitation: children with high average value are preferred. The second term is exploration: children with high prior probability and few visits get a bonus. `c_puct` controls the balance.

A larger `c_puct` makes the search explore more aggressively. A smaller `c_puct` makes it greedier. The right value depends on the noise in your value estimates; start near `sqrt(2)` and tune on benchmarks.

## Edge priors

The policy network supplies a prior probability for every legal edge. The prior is computed by evaluating the direction head, converting it to a softmax over legal directions, then for each direction evaluating the action head and converting it to a softmax over legal edges in that direction. The direction and action probabilities are multiplied and summed to get a single prior per edge.

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 315 — evaluate_edge_priors
fn evaluate_edge_priors(
    state: &GraphState,
    units: &Units,
    config: &PlannerConfig,
    edge_index: &PlanEdgeIndex,
    model: &PolicyBundle<TrainBackend>,
    device: &TrainDevice,
) -> (Vec<f32>, Vec<usize>) {
    // ... direction softmax over legal directions ...
    // ... action softmax for each direction over legal edges ...
    // ... combine into edge_prior[edge_idx] ...
}
```

This prior is what makes PUCT explore sensible edges first. Untrained networks still produce a prior, but it is essentially random; after training, the prior concentrates on high-value edges.

## Expansion

When the selected node has untried edges, the search expands the one with the highest prior:

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 172 — expansion inside search
let edge_idx = leaf
    .untried_edges
    .iter()
    .copied()
    .max_by(|a, b| {
        leaf.edge_priors[*a]
            .partial_cmp(&leaf.edge_priors[*b])
            .unwrap_or(std::cmp::Ordering::Equal)
    })
    .unwrap_or(leaf.untried_edges[0]);

leaf.untried_edges.retain(|&e| e != edge_idx);

match expand_edge(...) {
    Some(child_state) => { /* create child node */ }
    None => { /* expansion failed, treat as neutral value */ }
}
```

`expand_edge` resolves the selected edge into a concrete action using the power and squad heads, just like the one-step policy:

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 380 — expand_edge
fn expand_edge(
    state: &GraphState,
    edge_idx: usize,
    units: &Units,
    config: &PlannerConfig,
    edge_index: &PlanEdgeIndex,
    model: &PolicyBundle<TrainBackend>,
    device: &TrainDevice,
) -> Option<GraphState> {
    let edge = edge_index.get(edge_idx)?;
    // ... evaluate power and squad heads, resolve builders, execute action ...
}
```

If expansion fails (for example, because no idle builder is available), the search treats the result as a neutral value rather than crashing.

## Rollout evaluation

If a node is already fully expanded, or immediately after expanding a new child, the search estimates the leaf's value with a rollout. The rollout plays out the greedy hierarchical policy for up to `max_rollout_steps` simulator ticks, accumulating discounted per-step rewards plus a terminal bonus:

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 434 — rollout_value
fn rollout_value(
    state: &GraphState,
    goal: &UnitKind,
    units: &Units,
    config: &PlannerConfig,
    model: &PolicyBundle<TrainBackend>,
    max_steps: usize,
) -> f64 {
    // ... run macro_policy_plan greedily, accumulate discounted rewards ...
}
```

The rollout reuses the same `macro_policy_plan` function used by the one-step policy. This avoids duplicating inference logic and makes the search behave consistently whether `iterations` is zero or large.

## Backup

After expansion and rollout, the search adds the resulting value to `total_value` and increments `visits` for the leaf and every node on the selection path:

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 232 — backup
leaf.total_value += value;
leaf.visits += 1;
let mut current = &mut root;
current.total_value += value;
current.visits += 1;
for &child_idx in &path {
    current = &mut current.children[child_idx].1;
    current.total_value += value;
    current.visits += 1;
}
```

This is what makes UCB1/PUCT work: averages are updated, and exploration bonuses shrink as visit counts grow.

## Choosing the final action

After the iteration budget is exhausted, the search picks the root child with the **highest visit count**, not necessarily the highest average value. Visit count is a more robust signal because it reflects how much search effort was directed at the move.

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 246 — final action selection
let best_edge = root
    .children
    .iter()
    .max_by(|(_, a), (_, b)| a.visits.cmp(&b.visits))
    .map(|(edge_idx, _)| *edge_idx);
```

The selected edge is expanded one more time to produce the `SimAction` returned in the `PlanResult`.

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
    /// If true, always pick the highest-scoring plan-graph edge.
    deterministic: bool,
},
```

You can extend this later with a time budget and a parallel search worker.

## MCTS as a closed-loop planner

MCTS is the final piece that makes the system closed-loop:

```text
loop:
    action = mcts.search(state, goal)
    state.apply(action)
    if goal_reached(state, goal): break
```

Because the search is rooted in the current state and recomputes every tick, small deviations from the expected plan do not compound. The planner always reasons from the latest observation.

Now that the search is defined, we can wire it into the simulator actor loop and the CLI.
