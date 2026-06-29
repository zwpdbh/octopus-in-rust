# 3. The Macro-Direction Policy Network

The learned network is a **macro-direction policy**. It does not score individual build commands. Instead, it looks at the current economy and tech state and decides which of four high-level priorities to pursue next. A small deterministic resolver then turns that priority into a concrete `SelectionOption`.

This chapter explains why we use a two-level design, how `GraphState` is converted into network inputs, how the resolver works, and how the network is trained in Rust with `burn`.

## Why a macro-direction policy?

The previous design scored every legal `(state, candidate)` pair. That made the action space large and unit-specific: the same economy state could produce different scores depending on which exact mex or pgen happened to be a legal candidate. It also made greedy inference unstable, because a small change in candidate availability could flip the chosen action.

A macro-direction policy fixes this by splitting the decision:

1. **Learned layer:** given eco/state features, output a distribution over `{BuildPower, MoreMass, MorePower, TechUp}`.
2. **Rule-based resolver:** given the chosen direction and the current legal candidates, pick the most sensible concrete action.

The learned layer captures the economy-driven rhythm that transfers across units. The resolver handles the goal-specific details of which mex, which pgen, or which factory upgrade to use.

## From `PlanGraph` to candidates

The resolver still uses the legal candidates derived from the static `PlanGraph` and the current `GraphState`.

`SelectionPools::new` walks every edge in the plan graph and returns the legal options:

```rust
// crates/faf-sim/src/planner/mcts/selections.rs ~line 61 — SelectionPools::new
pub fn new(
    plan: &PlanGraph,
    state: &GraphState,
    units: &Units,
    config: &PlannerConfig,
) -> Self {
    // ...
}
```

The three option types are:

```rust
// crates/faf-sim/src/planner/mcts/selections.rs ~line 27 — SelectionOption enum
pub enum SelectionOption {
    /// Build a new unit of the given kind.
    Build(UnitKind),
    /// Upgrade an existing `from` unit into `to`.
    Upgrade { from: UnitKind, to: UnitKind },
    /// Assist an active project. Builders are resolved at execution time.
    Assist(NodeId),
}
```

The plan graph constrains the action space; the resolver chooses among graph-reachable next steps.

## Featurizing the state

The network consumes a fixed-size vector of length `STATE_FEATURE_COUNT`:

```rust
// crates/faf-sim/src/planner/mcts/features.rs ~line 16 — state feature count
pub const STATE_FEATURE_COUNT: usize = 13;
```

`state_features` extracts an economy and tech snapshot:

```rust
// crates/faf-sim/src/planner/mcts/features.rs ~line 44 — state_features
pub fn state_features(
    state: &GraphState,
    units: &Units,
    config: &PlannerConfig,
) -> Vec<f32> {
    // ... 13 scalar features ...
}
```

The features include:

- Net mass/energy income, scaled.
- Mass/energy storage ratios.
- Total active build power.
- Current simulation time.
- Fraction of max mex/pgen/energy-storage count already built.
- Number of active projects.
- Boolean tech milestones: T2 factory, T3 factory, T3 engineer.

Candidate-specific features are no longer fed into the network. The policy learns to prefer "more power" when energy is low, not to prefer "this specific pgen over that specific pgen."

## Network architecture

The policy network is a small MLP:

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 74 — MacroNet
#[derive(Module, Debug)]
pub struct MacroNet<B: Backend> {
    linear1: Linear<B>,
    activation: Relu,
    linear2: Linear<B>,
    output: Linear<B>,
}
```

It maps `STATE_FEATURE_COUNT -> 128 -> ReLU -> 64 -> ReLU -> MACRO_DIRECTION_COUNT`:

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 93 — MacroNet::new
pub fn new(device: &B::Device) -> Self {
    Self {
        linear1: LinearConfig::new(STATE_FEATURE_COUNT, 128).init(device),
        activation: Relu::new(),
        linear2: LinearConfig::new(128, 64).init(device),
        output: LinearConfig::new(64, MACRO_DIRECTION_COUNT).init(device),
    }
}
```

`MACRO_DIRECTION_COUNT` is 4:

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 34 — MacroDirection enum
pub enum MacroDirection {
    BuildPower = 0,
    MoreMass = 1,
    MorePower = 2,
    TechUp = 3,
}
```

`forward` accepts a `[batch, STATE_FEATURE_COUNT]` tensor and returns a `[batch, 4]` tensor — one logit for each macro direction.

## Turning scores into a policy

At decision time the planner computes state features, runs one forward pass, and either takes the argmax (greedy) or samples from the softmax over direction scores (stochastic):

```rust
// crates/faf-sim/src/planner/mcts/mod.rs ~line 85 — macro_policy_plan scoring and resolution
let scores = net.score_directions(&state, units, config, &device);

// Try directions in order of network preference until the resolver finds an
// executable candidate.
let mut direction_order: Vec<usize> = (0..scores.len()).collect();
if deterministic {
    direction_order.sort_by(|&a, &b| {
        scores[b]
            .partial_cmp(&scores[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
} else {
    direction_order = stochastic_direction_order(&scores);
}

for &dir_idx in &direction_order {
    let direction = MacroDirection::from_index(dir_idx).unwrap_or(MacroDirection::BuildPower);
    if let Some(option) = resolve_macro_direction(direction, &candidates, &state, units, &plan, config) {
        if let Some(action) = option.to_sim_action(&state, units) {
            return Ok(plan_result_with_action(state, action));
        }
    }
}
```

So the policy is:

```text
π(direction_i | state) = softmax(MacroNet(state))_i
action = resolve(direction_i, candidates, state)
```

If the top direction has no executable candidate, the planner falls back to the next-best direction. If no direction resolves, it issues `Wait`.

## The micro resolver

The resolver is deterministic and rule-based. It classifies each candidate by macro direction and then picks the best concrete action within the chosen direction.

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 190 — resolve_macro_direction
pub fn resolve_macro_direction(
    direction: MacroDirection,
    candidates: &[SelectionOption],
    state: &GraphState,
    units: &Units,
    plan: &PlanGraph,
    config: &PlannerConfig,
) -> Option<SelectionOption> {
    // ...
}
```

### Direction classification

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 52 — macro_direction_of
pub fn macro_direction_of(option: &SelectionOption, _state: &GraphState) -> MacroDirection {
    match option {
        SelectionOption::Assist(_) => MacroDirection::BuildPower,
        SelectionOption::Build(target) | SelectionOption::Upgrade { to: target, .. } => {
            macro_direction_of_kind(target)
        }
    }
}
```

`Assist` is always `BuildPower`. Builds and upgrades are classified by their target unit kind:

| Target kind | Direction |
|---|---|
| `Mex(_)`, `CapT2Mex`, `CapT3Mex` | `MoreMass` |
| `Pgen(_)`, `EnergyStorage` | `MorePower` |
| `Engineer(_)`, `Factory(T1)`, `Commander` | `BuildPower` |
| `Factory(T2)`, `Factory(T3)` | `TechUp` |

### Resolver rules

- `BuildPower`: if there are active projects and idle engineers, assist the most valuable active project; otherwise build the highest build-rate-per-mass engineer or factory.
- `MoreMass`: prefer the candidate with the highest incremental mass income per mass cost (mex upgrades, then new mexes, then capped mexes).
- `MorePower`: prefer the candidate with the highest incremental energy income per mass cost.
- `TechUp`: pick the cheapest factory build or upgrade that unlocks the next tier.

Tie-breakers are higher tier first, then shorter plan-graph distance to the goal.

## Training the network

The network is trained with **REINFORCE**, not supervised regression on completion time.

For each episode the trainer:

1. Starts from the ACU state.
2. Repeatedly computes state features, scores the four directions, and samples one.
3. Resolves the sampled direction to a concrete action and executes it.
4. Records the state features and chosen direction at every step.
5. Runs until the goal is reached or the step budget is exhausted.
6. Computes a shaped reward from the final state.

The reward encourages progress toward the goal:

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 525 — compute_progress_reward
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
// crates/faf-sim/src/planner/mcts/train.rs ~line 420 — Trainer::compute_returns
fn compute_returns(&mut self, episode: &mut Episode) {
    // Welford's online algorithm for running mean/variance
}
```

The policy gradient update maximizes the log-probability of the selected macro direction weighted by the normalized return, plus an entropy bonus:

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 450 — Trainer::update
let logits = self.model.forward(input).flatten::<1>(0, 1);
let log_probs = log_softmax(logits, 0);
let selected_log_prob = log_probs.clone().select(0, index_tensor);

let policy_loss = selected_log_prob.neg().mul(return_tensor);

let entropy = (probs * log_probs).neg().sum();
let entropy_loss = entropy.neg().mul_scalar(self.config.entropy_coef);

let loss = policy_loss + entropy_loss;
```

Configuration is controlled by `TrainConfig`:

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 36 — TrainConfig
pub struct TrainConfig {
    pub episodes: usize,
    pub max_steps: usize,
    pub dt: f64,
    pub learning_rate: f64,
    pub gamma: f32,
    pub epsilon: f32,
    pub epsilon_final: f32,
    pub epsilon_decay_episodes: usize,
    pub entropy_coef: f32,
    pub target_time: Option<f64>,
    pub verbose: bool,
}
```

- `episodes` — how many rollouts to run.
- `max_steps` — simulator steps per episode.
- `dt` — fixed timestep for rollouts.
- `learning_rate` — Adam step size.
- `gamma` — discount factor (reserved for future n-step returns; currently every step uses the final return).
- `epsilon` — probability of a random macro direction for exploration.
- `entropy_coef` — entropy bonus strength; higher values keep the policy spread out.

## Saving and loading

Trained weights are serialized with `burn`'s `CompactRecorder`:

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 650 — train_policy
pub fn train_policy(
    units: &Units,
    goal: &UnitKind,
    config: TrainConfig,
) -> (MacroNet<TrainBackend>, Option<MacroNet<TrainBackend>>, TrainStats) {
    let mut trainer = Trainer::new(config);
    let stats = trainer.train(units, goal);
    let best_model = trainer.best_model.take();
    let model = trainer.into_model();
    (model, best_model, stats)
}
```

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 671 — save_model
pub fn save_model(model: &MacroNet<TrainBackend>, path: &std::path::Path) -> Result<(), String> {
    // ...
}
```

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 686 — load_model
pub fn load_model(path: &std::path::Path) -> Result<MacroNet<TrainBackend>, String> {
    // ...
}
```

`load_model` validates that the saved model's input dimension matches `STATE_FEATURE_COUNT`. A mismatch means the featurization code changed and the model must be retrained.

The same `MacroNet` struct is used for training (`Autodiff<NdArray>`) and inference (`NdArray` once converted), so no conversion is needed.

## Future: value head and UCT

The current network is a pure policy over macro directions. A future extension is to add a second output head that estimates state value `V(state)` for use inside UCT:

```text
UCT(child) = (child.total_value / child.visits)
             + c_puct * prior(child) * sqrt(parent.visits) / (1 + child.visits)
```

The policy output would become the prior over macro directions and the value output would replace the hand-shaped reward for leaf evaluation. The resolver would still turn the chosen direction into a concrete action.
