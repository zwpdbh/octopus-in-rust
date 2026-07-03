# 7. MCTS Search with a Burn Policy Prior

During training the policy acts alone. During simulation we wrap it in **Monte Carlo Tree Search (MCTS)** to look ahead. MCTS keeps a search tree rooted in the current state, explores promising directions, and returns the action with the highest visit count.

This chapter explains UCT/PUCT selection, 6-way expansion, how the Burn network supplies prior probabilities, and how greedy policy rollouts estimate leaf values.

## Why MCTS over directions?

The policy outputs a distribution over six high-level directions. MCTS therefore searches over those same six directions. Each tree edge is a direction, not a concrete build order. The heuristic layer resolves a direction into a `SimAction` only when the edge is expanded.

This has two benefits:

1. **Small branching factor.** Six is much smaller than the number of concrete plan-graph edges, so the tree stays manageable.
2. **Reuses the trained network.** The same `evaluate_direction` call provides both the prior probabilities for selection and the greedy policy for rollouts.

## Configuration

`MctsConfig` controls the search budget:

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 22 — MctsConfig
pub struct MctsConfig {
    pub iterations: usize,
    pub c_puct: f64,
    pub max_rollout_steps: usize,
}
```

- `iterations` — how many selection/expansion/rollout/backup loops to run.
- `c_puct` — UCT exploration constant. Higher values explore more.
- `max_rollout_steps` — maximum length of a rollout from a leaf.

## Tree nodes

Each MCTS node stores the simulator state at that node, visit statistics, and the legal directions that have not yet been expanded:

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 42 — MctsNode
struct MctsNode {
    state: SimulationState,
    total_value: f64,
    visits: usize,
    children: Vec<(usize, Box<MctsNode>)>,
    untried_directions: Vec<usize>,
    direction_priors: Vec<f32>,
    is_terminal: bool,
}
```

When a node is created, the policy network evaluates the state and produces a prior probability for each legal direction:

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 275 — evaluate_direction_priors
fn evaluate_direction_priors(
    state: &SimulationState,
    goal: &Goal,
    units: &Units,
    config: &PlannerConfig,
    model: &dyn ValueNet,
    plan: &PlanGraph,
) -> (Vec<f32>, Vec<usize>) {
    let features = state_features(state, units, config);

    let mut direction_logits = model.evaluate_direction(features);
    let direction_mask = legal_direction_mask(state, units, config, goal, plan);
    apply_mask(&mut direction_logits, &direction_mask);
    let direction_probs = softmax_probs(&direction_logits);

    let untried: Vec<usize> = direction_probs
        .iter()
        .enumerate()
        .filter(|(_, &p)| p > 0.0)
        .map(|(i, _)| i)
        .collect();

    (direction_probs, untried)
}
```

The prior probabilities come from the trained Burn network. Illegal directions are masked to near-zero probability before softmax.

## Selection with PUCT

Selection walks down the tree from the root to a leaf using the PUCT formula. PUCT is like UCB1 but incorporates the learned prior:

```text
score(child) = Q(child) + c_puct * prior(child) * sqrt(parent_visits) / (1 + child_visits)
```

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 239 — select_path
fn select_path(node: &MctsNode, c_puct: f64) -> Vec<usize> {
    let mut path = Vec::new();
    let mut current = node;

    while !current.is_terminal
        && current.untried_directions.is_empty()
        && !current.children.is_empty()
    {
        let parent_visits = current.visits as f64;
        let best = current
            .children
            .iter()
            .enumerate()
        .map(|(i, (direction_idx, child))| {
            let prior = current.direction_priors[*direction_idx] as f64;
            let q = if child.visits == 0 {
                0.0
            } else {
                child.total_value / child.visits as f64
            };
            let u = c_puct * prior * parent_visits.sqrt() / (1.0 + child.visits as f64);
            (i, q + u)
        })
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
        .unwrap_or(0);

        path.push(best);
        current = &current.children[best].1;
    }

    path
}
```

The prior term `u` is large when:

- the network assigns high probability to the direction,
- the parent has been visited many times,
- the child has been visited few times.

This biases exploration toward directions the network thinks are good, while still allowing visits to low-prior directions if their empirical value `q` becomes high.

## Expansion

When selection reaches a node with untried directions, MCTS expands the highest-prior untried direction:

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 331 — expand_direction
fn expand_direction(
    state: &SimulationState,
    direction_idx: usize,
    goal: &Goal,
    units: &Units,
    config: &PlannerConfig,
    plan: &PlanGraph,
) -> Option<SimulationState> {
    let direction = EdgeCategory::ALL.get(direction_idx)?;
    let action = direction_to_action(*direction, state, units, config, goal, plan);
    if matches!(action, SimAction::Wait) {
        return None;
    }

    let mut new_state = state.clone();
    if execute_action(&mut new_state, &action, units, config.dt).is_err() {
        return None;
    }

    Some(new_state)
}
```

Expansion fails if the chosen direction maps to `SimAction::Wait` (no legal concrete action) or if the simulator rejects the action. In that case the expansion contributes a neutral value of `0.0`.

## Rollouts

If a leaf is fully expanded but not terminal, MCTS estimates its value by running a **rollout**: a greedy policy playout from the leaf state until the goal is reached or a step limit is hit.

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 355 — rollout_value
fn rollout_value(
    state: &SimulationState,
    goal: &Goal,
    units: &Units,
    config: &PlannerConfig,
    model: &dyn ValueNet,
    max_steps: usize,
    plan: &PlanGraph,
) -> f64 {
    let mut s = state.clone();
    let mut total = 0.0f32;
    let mut discount = 1.0f32;
    let gamma = 0.99f32;

    for _ in 0..max_steps {
        if s.goal_reached(goal) {
            break;
        }

        let prev = s.clone();

        let action = greedy_direction_action(&s, goal, units, config, model, plan);
        if execute_action(&mut s, &action, units, config.dt).is_err() {
            s.tick(units, config.dt);
        }

        total += discount * compute_step_reward(&prev, &s, units);
        discount *= gamma;
    }

    let terminal = compute_terminal_bonus(&s, s.goal_reached(goal));
    (total + discount * terminal) as f64
}
```

The rollout uses the same network as the tree, but greedily (`masked_argmax`) instead of sampling. This gives a deterministic value estimate that can be averaged across many MCTS iterations.

## Backup

After expansion or rollout, the resulting value is added to every node on the selection path:

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 200 — backup
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

Backup is what makes MCTS a multi-step lookahead: every node accumulates the outcomes of the futures explored below it.

## Choosing the final action

After all iterations, the planner picks the root child with the highest visit count:

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 214 — final action selection
let best_direction = root
    .children
    .iter()
    .max_by(|(_, a), (_, b)| a.visits.cmp(&b.visits))
    .map(|(direction_idx, _)| *direction_idx);
```

Visit count is more robust than raw value because it incorporates both the network's prior and the empirical quality discovered during search.

## The full search function

The pieces come together in `MctsSearch::search`:

```rust
// crates/faf-sim/src/planner/mcts/search.rs ~line 101 — MctsSearch::search
pub fn search(
    &self,
    initial_state: SimulationState,
    goal: &Goal,
    units: &Units,
    planner_config: &PlannerConfig,
    model: &dyn ValueNet,
) -> Result<PlanResult, PlannerError> {
    let plan = build_plan_graph(units, *goal);
    let mut root = MctsNode::new(initial_state, goal, units, planner_config, model, &plan);

    // If no legal actions from root, just wait.
    if root.untried_directions.is_empty() && !root.is_terminal {
        let mut state = root.state.clone();
        state.tick(units, planner_config.dt);
        return Ok(plan_result_with_action(state, SimAction::Wait));
    }

    for _ in 0..self.config.iterations {
        let path = select_path(&root, self.config.c_puct);

        // Walk to the selected leaf.
        let mut leaf = &mut root;
        for &child_idx in &path {
            leaf = &mut leaf.children[child_idx].1;
        }

        let value = if leaf.is_terminal {
            compute_terminal_bonus(&leaf.state, true) as f64
        } else if leaf.untried_directions.is_empty() {
            rollout_value(&leaf.state, goal, units, planner_config, model, self.config.max_rollout_steps, &plan)
        } else {
            // Expand one untried direction, then rollout from the child.
            // ...
        };

        // Backup value along the path.
        // ...
    }

    // Pick the root child with the highest visit count.
    // ...
}
```

If `iterations` is zero, the loop body is skipped and the search picks the root child with the highest prior probability (because no visits have occurred yet). This is a cheap one-step policy fallback.

## MCTS vs training

| | Training | MCTS |
| --- | --- | --- |
| **Action selection** | Sample from the masked policy, with epsilon-greedy noise. | PUCT selection over the tree, then greedy argmax in rollouts. |
| **Tree** | None. | Maintains a tree rooted in the current state. |
| **Network use** | One forward pass per step. | One forward pass per node expansion plus one per rollout step. |
| **Goal** | Generate gradients. | Pick the best action from the current observed state. |

## When MCTS helps

MCTS is not magic. If the trained policy has never learned to build a T3 engineer, MCTS cannot invent that path because its priors and rollouts both come from the policy. MCTS amplifies a good policy into better move selection; it cannot fix a bad one.

MCTS is most useful when:

- the policy is roughly right but makes local mistakes,
- a small short-term sacrifice leads to a large long-term gain,
- the simulator state has drifted and the policy's greedy choice is no longer optimal.

With MCTS in place, the next chapter shows how the planner is wired into the CLI and actor loop.
