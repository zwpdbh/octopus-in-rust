# 3. The Policy Network

The learned network is a **policy network**, not a value network. It does not predict remaining time. Instead, it scores every legal `(state, candidate)` pair and the planner samples the next action from a softmax over those scores.

This chapter explains why we use a policy, how `GraphState` and a `PlanGraph` candidate are converted into network inputs, and how the network is trained in Rust with `burn`.

## Why a policy network?

Classic MCTS evaluates a leaf by playing random moves to the end of the episode and averaging the outcome. For FAF this is wasteful:

- The horizon is long; a single rollout may take thousands of ticks.
- Random build orders are almost always terrible, so the average is noisy.
- The reward is sparse: you learn nothing until the goal finishes.

A policy network avoids random rollouts by directly predicting which candidate action is promising in the current state. The current planner uses it as a one-step stochastic policy; full UCT search can be layered on top later while reusing the same network.

## From `PlanGraph` to candidates

The network never sees the full unit roster. It sees only the legal candidates derived from the static `PlanGraph` and the current `GraphState`.

`SelectionPools::derive` walks every edge in the plan graph:

```rust
// crates/faf-sim/src/planner/mcts/selections.rs ~line 81 — SelectionPools::derive
pub fn derive(plan: &PlanGraph, state: &GraphState, units: &Units) -> Self {
    // ...
}
```

An option is produced when the edge source is owned/active, the target is not yet owned or under construction, and a capable idle builder exists. The three option types are:

```rust
// crates/faf-sim/src/planner/mcts/selections.rs ~line 18 — SelectionOption enum
pub enum SelectionOption {
    /// Build a new unit of the given kind.
    Build(UnitKind),
    /// Upgrade an existing `from` unit into `to`.
    Upgrade { from: UnitKind, to: UnitKind },
    /// Assist an active project. Builders are resolved at execution time.
    Assist(NodeId),
}
```

The plan graph therefore constrains the action space: the model learns to choose among graph-reachable next steps, not among arbitrary units.

## Featurizing the state and candidate

The network consumes a fixed-size vector of length `FEATURE_COUNT`:

```rust
// crates/faf-sim/src/planner/mcts/features.rs ~line 15 — feature counts
pub const STATE_FEATURE_COUNT: usize = 12;
pub const CANDIDATE_FEATURE_COUNT: usize = 12;
pub const FEATURE_COUNT: usize = STATE_FEATURE_COUNT + CANDIDATE_FEATURE_COUNT;
```

### State features

`state_features` extracts an economy and tech snapshot:

```rust
// crates/faf-sim/src/planner/mcts/features.rs ~line 25 — state_features
pub fn state_features(
    state: &GraphState,
    _goal_id: &UnitKind,
    units: &Units,
    config: &PlannerConfig,
) -> Vec<f32> {
    // ... 12 scalar features ...
}
```

The features include:

- Net mass/energy income, scaled.
- Mass/energy storage ratios.
- Total active build power.
- Current simulation time.
- Fraction of max mex/pgen count already built.
- Number of active projects.
- Boolean tech milestones: T2 factory, T3 factory, T3 engineer.

### Selection-option features

`candidate_features` describes the action itself and its relationship to the goal:

```rust
// crates/faf-sim/src/planner/mcts/features.rs ~line 76 — candidate_features
pub fn candidate_features(
    candidate: &SelectionOption,
    state: &GraphState,
    plan: &PlanGraph,
    units: &Units,
) -> Vec<f32> {
    // ... 12 scalar features ...
}
```

The features include:

- One-hot-ish flags for action type: build / upgrade / assist.
- Tech tier of the target, normalized to `[0, 1]`.
- Boolean unit-category flags: Mex, Pgen, Factory, Engineer, Unique.
- Build cost (mass/energy), scaled.
- Shortest-path distance from the candidate target to the goal in the `PlanGraph`.

This distance feature is the main way graph topology enters the model: candidates closer to the goal receive a less-penalized input value.

### Combined input

For each candidate the planner concatenates the state vector and the candidate vector:

```rust
// crates/faf-sim/src/planner/mcts/features.rs ~line 129 — featurize
pub fn featurize(
    state: &GraphState,
    candidate: &SelectionOption,
    goal_id: &UnitKind,
    units: &Units,
    plan: &PlanGraph,
    config: &PlannerConfig,
) -> Vec<f32> {
    let mut features = state_features(state, goal_id, units, config);
    features.extend(candidate_features(candidate, state, plan, units));
    debug_assert_eq!(features.len(), FEATURE_COUNT);
    features
}
```

All values are clamped to a reasonable range so the network trains reliably without extreme inputs.

## Network architecture

The policy network is a small MLP:

```rust
// crates/faf-sim/src/planner/mcts/value_net.rs ~line 23 — ValueNet
#[derive(Module, Debug)]
pub struct ValueNet<B: Backend> {
    linear1: Linear<B>,
    activation: Relu,
    linear2: Linear<B>,
    output: Linear<B>,
}
```

It maps `FEATURE_COUNT -> 256 -> ReLU -> 128 -> ReLU -> 1`:

```rust
// crates/faf-sim/src/planner/mcts/value_net.rs ~line 35 — ValueNet::new
pub fn new(device: &B::Device) -> Self {
    Self {
        linear1: LinearConfig::new(FEATURE_COUNT, 256).init(device),
        activation: Relu::new(),
        linear2: LinearConfig::new(256, 128).init(device),
        output: LinearConfig::new(128, 1).init(device),
    }
}
```

`forward` accepts a `[batch, FEATURE_COUNT]` tensor and returns a `[batch, 1]` tensor — one scalar preference score for each candidate.

```rust
// crates/faf-sim/src/planner/mcts/value_net.rs ~line 45 — ValueNet::forward
pub fn forward(&self, features: Tensor<B, 2>) -> Tensor<B, 2> {
    let x = self.linear1.forward(features);
    let x = self.activation.forward(x);
    let x = self.linear2.forward(x);
    let x = self.activation.forward(x);
    self.output.forward(x)
}
```

## Turning scores into a policy

At decision time the planner builds a feature matrix of shape `[n_candidates, FEATURE_COUNT]`, runs one forward pass, and samples from the softmax distribution over scores:

```rust
// crates/faf-sim/src/planner/mcts/mod.rs ~line 87 — mlp_policy_plan scoring and sampling
let scored = net.score_candidates(&state, &candidates, goal_id, units, &plan, config, &device);

// Keep only candidates that can actually be executed now and sample from them.
let mut executable = Vec::new();
let mut scores = Vec::new();
for (candidate, score) in scored {
    if candidate_to_action(&candidate, &state, units, &plan).is_some() {
        executable.push(candidate);
        scores.push(score);
    }
}

let probs = softmax(&scores, SAMPLE_TEMPERATURE);
let dist = WeightedIndex::new(&probs)
    .map_err(|e| PlannerError::Other(format!("invalid policy distribution: {}", e)))?;
let idx = dist.sample(&mut rng);
let chosen = &executable[idx];
```

So the policy is:

```text
π(candidate_i | state) = softmax(MLP(state, candidate_i))_i
```

Only candidates that survive the executable filter are sampled; if none are executable the planner issues `Wait`.

## Training the network

The network is trained with **REINFORCE**, not supervised regression on completion time.

For each episode the trainer:

1. Starts from the ACU state.
2. Repeatedly derives `SelectionPools`, scores candidates, and samples an action.
3. Records the chosen action and the feature matrix at every step.
4. Runs until the goal is reached or the step budget is exhausted.
5. Computes a shaped reward from the final state.

The reward encourages progress toward the goal:

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 338 — compute_progress_reward
fn compute_progress_reward(
    state: &GraphState,
    units: &Units,
    goal: &UnitKind,
    plan: &PlanGraph,
) -> f32 {
    // ...
}
```

It includes:

- Points for owning nodes on the plan graph, weighted inversely by distance to the goal.
- Bonuses for unlocking higher factory and engineer tech tiers.
- Rewards for economy scale (build power, mass/energy income).
- A large bonus for reaching the goal, plus a time premium for faster completion.
- A small penalty for failing to reach the goal within the step budget.

Because the reward is computed from the final state only, every step in the episode receives the same raw return. A running mean and variance baseline is maintained across episodes for normalization:

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 249 — Trainer::compute_returns
fn compute_returns(&mut self, episode: &mut Episode) {
    // Welford's online algorithm for running mean/variance
}
```

The policy gradient update maximizes the log-probability of the selected candidate weighted by the normalized return, plus an entropy bonus:

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 273 — Trainer::update
fn update(&mut self, episode: &Episode) -> f32 {
    // ... log_probs.select(action_index) * return + entropy bonus ...
}
```

Configuration is controlled by `TrainConfig`:

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 35 — TrainConfig
pub struct TrainConfig {
    pub episodes: usize,
    pub max_steps: usize,
    pub dt: f64,
    pub learning_rate: f64,
    pub gamma: f32,
    pub epsilon: f32,
    pub entropy_coef: f32,
}
```

- `episodes` — how many rollouts to run.
- `max_steps` — simulator steps per episode.
- `dt` — fixed timestep for rollouts.
- `learning_rate` — Adam step size.
- `gamma` — discount factor (reserved for future n-step returns; currently every step uses the final return).
- `epsilon` — probability of a random action for exploration.
- `entropy_coef` — entropy bonus strength; higher values keep the policy spread out.

## Saving and loading

Trained weights are serialized with `burn`'s `CompactRecorder`:

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 413 — train_policy
pub fn train_policy(
    units: &Units,
    goal: &UnitKind,
    config: TrainConfig,
) -> (ValueNet<TrainBackend>, TrainStats) {
    let mut trainer = Trainer::new(config);
    let stats = trainer.train(units, goal);
    let model = trainer.into_model();
    (model, stats)
}
```

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 425 — save_model
pub fn save_model(model: &ValueNet<TrainBackend>, path: &std::path::Path) -> Result<(), String> {
    // ...
}
```

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 436 — load_model
pub fn load_model(path: &std::path::Path) -> Result<ValueNet<TrainBackend>, String> {
    // ...
}
```

The same `ValueNet` struct is used for training (`Autodiff<NdArray>`) and inference (`NdArray` once converted), so no conversion is needed.

## Future: value head and UCT

The current network is a pure policy. A future extension is to add a second output head that estimates state value `V(state)` for use inside UCT:

```text
UCT(child) = (child.total_value / child.visits)
             + c_puct * prior(child) * sqrt(parent.visits) / (1 + child.visits)
```

The policy output would become the prior and the value output would replace the hand-shaped reward for leaf evaluation. That change keeps the same input featurization and network body; only the final layer and loss change.
