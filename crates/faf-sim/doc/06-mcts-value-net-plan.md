# 6. Plan: MCTS with a Learned Value Network

> We will replace the open-loop beam search with a closed-loop planner that
> thinks at decision time. A neural network estimates how good any state is,
> and Monte Carlo Tree Search (MCTS) focuses computation on the most promising
> futures.

## Why this direction

The current planner is **open-loop**: `Planner::plan` generates a full sequence
of actions up front, and the reactive actor executes the first action. If the
simulator state drifts from the plan — because of rounding, stalls, or future
choices — the planner has no way to recover until the next full replan.

MCTS is **closed-loop**. It keeps a search tree rooted in the *current observed
state*. At every decision it expands the tree, evaluates leaves with the value
network, and picks the action with the best statistics. The plan is recomputed
from the real state each tick, so small deviations do not compound.

This also matches the project's long-term goal: a learning system that can
discover non-obvious build orders. The value network learns from millions of
simulated rollouts, while MCTS turns that knowledge into strong decisions.

## High-level architecture

```text
┌─────────────────┐     Observation      ┌──────────────────┐
│   SimActor      │ ───────────────────> │   PlannerActor   │
│  (environment)  │                      │  (runs MCTS)     │
└─────────────────┘                      └────────┬─────────┘
       ^                                          │
       │                                          │
       │ Command                                  │ value / policy
       │                                          ▼
       │                                 ┌──────────────────┐
       └──────────────────────────────── │  Neural Network  │
                                         │  (value net)     │
                                         └──────────────────┘
```

- **Environment:** existing `sim::GraphState` and `SimActor`.
- **State:** `GraphState` plus the goal unit(s).
- **Actions:** `Build`, `Assist`, `Wait` from `planner::search::SearchAction`.
- **Value network:** predicts the negative remaining time to reach the goal,
  or a normalized "win" probability.
- **MCTS:** selects actions, expands nodes, rolls out to terminal or a depth
  budget, and backs up values.

## Phased implementation

### Phase 0 — Baseline and instrumentation

Before changing the planner, lock down measurement.

1. Add a benchmark harness that runs the existing beam planner against a
   fixed set of goals (T1 pgen, T1 factory + engineer, T2 factory, T3
   engineer, Monkeylord).
2. Record:
   - completion time,
   - number of search steps,
   - wall-clock planning time,
   - final economy state.
3. Add a way to serialize `(state, action, next_state, reward)` trajectories
   from beam-search runs for later training data.

**Deliverable:** a `planner_bench` binary and a corpus of baseline trajectories.

### Phase 1 — State featurization

The value network cannot eat a `GraphState` directly. We need a fixed-size
feature vector or a small graph encoding.

**Option A: hand-crafted feature vector (recommended first)**

```rust
// docref: example
pub struct FafStateFeatures {
    // Time and progress
    pub current_time: f64,
    pub goal_completed: bool,

    // Economy
    pub mass_income: f64,
    pub energy_income: f64,
    pub mass_storage_ratio: f64,
    pub energy_storage_ratio: f64,

    // Builder summary
    pub idle_engineer_count: i32,
    pub busy_engineer_count: i32,
    pub factory_count_by_tech: [i32; 3],

    // Goal distance
    pub goal_tech_level: i32,
    pub missing_tech_prereqs: i32,
    pub owned_prereq_units: i32,

    // Recent events
    pub energy_stalled_last_tick: bool,
    pub mass_stalled_last_tick: bool,
}
```

**Option B: graph neural network (stretch)**

Encode the build graph directly with node features (unit type, completion
status, build power) and edges (builder -> target). A GNN is more expressive
but harder to train and slower. We will revisit it once Option A works.

**Deliverable:** `planner::mcts::state_features::featurize(state, goal) -> Vec<f32>`.

### Phase 2 — Value network

Train a small feed-forward network to predict the outcome of a state.

**Target:**

```text
value(state) = -remaining_time_to_goal / time_scale
```

where `time_scale` is a constant (e.g., 600 seconds for a Monkeylord) so the
network sees numbers near `[-1, 0]`.

**Architecture (hand-crafted features):**

```text
input (N features)
  -> linear -> relu -> dropout
  -> linear -> relu
  -> linear -> tanh -> output (1 scalar)
```

**Training data:**

1. Run the beam planner or a random policy on many goals.
2. For every visited `GraphState`, record the true final completion time.
3. Train with mean-squared error (MSE) loss.

This is supervised learning, not RL, so it is stable and debuggable.

**Library choice:** `candle` or `burn`. Both run in pure Rust. Start with
whichever has the simpler MSE regression example.

**Deliverable:**
- `planner::mcts::ValueNet` struct wrapping the neural network.
- A training script that exports a checkpoint.
- Inference code that loads the checkpoint inside `faf-sim`.

### Phase 3 — MCTS planner

Implement UCT-style MCTS that uses the value network for leaf evaluation.

**Node:**

```rust
// docref: example
pub struct MctsNode {
    pub state: GraphState,
    pub parent: Option<NodeId>,
    pub action_from_parent: Option<SearchAction>,
    pub children: Vec<NodeId>,
    pub untried_actions: Vec<SearchAction>,
    pub visits: u32,
    pub total_value: f64,
}
```

**Four MCTS steps:**

1. **Select:** traverse from root using UCT until a leaf or partially expanded
   node is reached.
   ```text
   UCT = (child.total_value / child.visits)
         + c_puct * policy_prior * sqrt(parent.visits) / (1 + child.visits)
   ```
   For Phase 3 we omit the policy prior (`c_puct * ...`) and use a simple UCB1
   bonus. The policy prior is added in Phase 4.

2. **Expand:** generate successors of the selected node using the existing
   `SearchConfig::successors` function. Add them as children.

3. **Evaluate:** if a child is terminal, use the true outcome. Otherwise,
   featurize the state and ask the value network.

4. **Backup:** add the evaluated value to every node on the path from the
   selected node back to the root.

**Decision:** after a fixed number of iterations or a time budget, pick the
root child with the highest visit count (most robust, not just highest value).

**Deliverable:** `Planner::with_config(Strategy::Mcts { ... }, ...)` and a
working `PlannerActor` that calls it each tick.

### Phase 4 — Policy prior (AlphaZero-style MCTS)

Add a second network head that outputs action probabilities. The prior guides
which children to explore first.

**Input:** same features as the value net.
**Output:** a probability for every legal action in the current state.

Because the action space is variable-size, use action masking:

1. Generate all legal actions with `SearchConfig::successors`.
2. Assign indices to the fixed "action vocabulary" (e.g., top-K buildable unit
   IDs, assist each active project, wait).
3. Mask illegal indices to `-inf` before softmax.

**Training:** after each MCTS search, the visit counts of the root children
become the training target for the policy. The value target is the outcome of
the rollout.

**Deliverable:** a combined `ValuePolicyNet` and a self-play training loop.

### Phase 5 — Self-play and improvement

Close the loop:

1. Run MCTS with the current network to generate games.
2. Store `(state, policy_target, value_target)` tuples.
3. Train the network on the new data.
4. Evaluate the new network against the previous one.
5. Keep the winner.

This is the AlphaZero recipe, adapted to a single-player optimization problem.

## Integration with the existing planner enum

Add `Strategy::Mcts` alongside `Greedy` and `Beam`:

```rust
// crates/faf-sim/src/planner/core.rs ~line 75 — Strategy enum (abbreviated)
pub enum Strategy {
    Greedy,
    Beam { beam_width: usize },
    Mcts {
        iterations: usize,
        c_puct: f64,
        time_budget_ms: u64,
    },
}
```

`Planner::plan` dispatches to `mcts::plan` when `Strategy::Mcts` is selected.
The CLI can pick it with a new argument or default to MCTS once it beats the
beam baseline.

## Evaluation plan

At every phase, compare against the beam-search baseline on the benchmark
suite. The primary metric is completion time. Secondary metrics:

- wall-clock planning time per decision,
- number of simulator ticks per decision,
- robustness across random starting perturbations (e.g., small `dt` changes).

A result is only accepted if it is **faster or equal on average** and **not
orders of magnitude slower**.

## Risks and mitigations

| Risk | Mitigation |
|------|------------|
| Value net is inaccurate for unseen states. | Start with supervised training on beam data; keep a fallback heuristic. |
| MCTS is too slow per decision. | Limit iterations, cache the tree between ticks, reuse subtrees. |
| Action space explodes. | Use action masking and a fixed vocabulary; prune obviously bad actions. |
| Training data is biased. | Generate data from multiple strategies and random goals. |
| Rust ML library limitations. | Begin with the simplest library; be ready to export to ONNX if needed. |

## What we do not do yet

- **Multi-goal planning:** keep the current single-goal focus.
- **Opponent modeling:** no enemy, no fog of war.
- **Full game bot:** build-order optimization only.
- **Real-time constraints:** optimize offline first, then measure speed.

## Next steps

1. Approve this plan and pick Phase 0 goals.
2. Decide the first benchmark suite (suggested: T1 pgen, T1 factory + engineer,
   T2 factory, T3 engineer, Monkeylord).
3. Choose the Rust ML library (`candle` or `burn`).
4. Create a tracking issue and start Phase 0.
