# 6. Training Pipeline

The macro-direction policy is trained with **REINFORCE**: roll out episodes with the current policy, then update the network so that macro directions leading to higher rewards become more likely.

## Single-phase policy-gradient training

Unlike a supervised warm-start that regresses on completion time, the current trainer learns directly from its own rollouts. The loop is:

1. Start a fresh `GraphState` with only the ACU.
2. Run the current policy for up to `max_steps` simulator ticks.
3. At each step, compute state features, score the four macro directions, and sample one.
4. Resolve the sampled direction to a concrete action and execute it.
5. Record the state features and selected direction at every step.
6. Compute a shaped reward from the final state.
7. Update the network with REINFORCE plus an entropy bonus.

This is implemented in `Trainer::train`:

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 135 — Trainer::train
pub fn train(&mut self, units: &Units, goal: &UnitKind) -> TrainStats {
    let plan = units
        .plan_graph(goal)
        .expect("goal must be reachable for training");
    // ... run episodes and update ...
}
```

## Configuration

`TrainConfig` controls the training run:

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

| Field | Default | Role |
|---|---|---|
| `episodes` | 200 | Number of rollouts to run. Use `0` to run until `target_time` is hit or the process is interrupted. |
| `max_steps` | 500 | Maximum simulator ticks per episode. |
| `dt` | 1.0 | Fixed simulator timestep for rollouts. |
| `learning_rate` | 1e-3 | Adam step size. |
| `gamma` | 0.99 | Discount factor (reserved for future n-step returns). |
| `epsilon` | 0.1 | Initial probability of taking a random macro direction. |
| `epsilon_final` | 0.1 | Final epsilon after decay. Same as `epsilon` when no decay. |
| `epsilon_decay_episodes` | same as `episodes` | Episodes over which to linearly decay `epsilon` to `epsilon_final`. `0` disables decay. |
| `entropy_coef` | 0.01 | Entropy bonus strength. |
| `target_time` | `None` | Stop early when an episode reaches the goal in at most this many seconds. |
| `verbose` | `false` | Print per-episode progress to stderr. |

The network input is `STATE_FEATURE_COUNT` economy/state features. Saved model checkpoints from before the macro-direction redesign will fail to load and must be retrained.

## Rollout details

Each episode begins with a fresh state:

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 290 — Trainer::run_episode
let mut state = GraphState::new(units, &[UnitKind::Commander]);
```

At each tick:

1. Derive `SelectionPools` from the `PlanGraph`.
2. Compute `state_features`.
3. With probability `epsilon`, pick a random macro direction; otherwise sample from the softmax over network scores.
4. Resolve the chosen direction to a concrete `SelectionOption`.
5. Convert the option to a `SimAction` and execute it.
6. If the resolver cannot find an executable candidate, issue `Wait`.

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 310 — direction selection and resolution
let state_feats = state_features(&state, units, planner_config);
let scores = self.model.evaluate_single(state_feats.clone(), &self.device);

let direction_index = if self.rng.gen::<f32>() < epsilon {
    self.rng.gen_range(0..MACRO_DIRECTION_COUNT)
} else {
    sample_direction_index(&scores, &mut self.rng)
};
let direction = MacroDirection::from_index(direction_index)
    .unwrap_or(MacroDirection::BuildPower);

let selected = match resolve_macro_direction(
    direction,
    &candidates,
    &state,
    units,
    plan,
    planner_config,
) {
    Some(option) => option,
    None => {
        state.tick(units, self.config.dt);
        continue;
    }
};
```

## Reward shaping

Because the raw reward — reaching the goal — is sparse, the trainer uses a shaped reward computed from the final state of each episode:

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

The reward includes:

- Points for owning nodes on the plan graph, weighted inversely by distance to the goal.
- Bonuses for unlocking higher factory and engineer tech tiers.
- Rewards for economy scale (build power, mass/energy income).
- A large bonus for reaching the goal, plus a time premium for faster completion.
- A small penalty for failing to reach the goal within the step budget.

This shaped reward is the same for every step in the episode because the final state captures the entire trajectory's outcome.

## Baseline normalization

REINFORCE needs a baseline to reduce variance. Because the reward is computed from the final state only, a per-episode mean would collapse to zero. Instead, the trainer maintains a **running mean and variance** of episode returns across training:

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 420 — Trainer::compute_returns
fn compute_returns(&mut self, episode: &mut Episode) {
    // Welford's online algorithm
    self.return_count += 1.0;
    let delta = raw_return - self.return_mean;
    self.return_mean += delta / self.return_count;
    // ...
    let normalized = (raw_return - self.return_mean) / std;

    for step in &mut episode.steps {
        step.return_value = normalized;
    }
}
```

Each step's target is the normalized return, which tells the policy whether this episode was better or worse than the trainer's historical average.

If variance remains high, a future extension is to add a learned state-value baseline `V(state)` and train it with MSE against observed returns. The policy gradient would then use advantage `return − V(state)`.

## Policy update

For each recorded step, the trainer:

1. Forwards the state feature vector through the macro network.
2. Applies `log_softmax` over the four direction logits.
3. Selects the log-probability of the direction that was taken.
4. Multiplies by the normalized return.
5. Adds an entropy bonus to encourage exploration.
6. Backpropagates and applies Adam.

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 450 — Trainer::update
let logits = self.model.forward(input).flatten::<1>(0, 1);
let log_probs = log_softmax(logits, 0);
let selected_log_prob = log_probs.clone().select(0, index_tensor);

let policy_loss = selected_log_prob.neg().mul(return_tensor);

let entropy = (probs * log_probs).neg().sum();
let entropy_loss = entropy.neg().mul_scalar(self.config.entropy_coef);

let loss = policy_loss + entropy_loss;

let grads = loss.backward();
// ... optimizer step ...
```

## Curriculum and data diversity

The policy can overfit to the exact goals and starting conditions it trains on. To improve generalization:

- Train on a curriculum of goals: T1 pgen, T1 factory + engineer, T2 factory, T3 engineer, Monkeylord.
- Vary `max_steps` and `dt` between runs.
- Use a non-zero `epsilon` throughout training so the policy continues to explore.
- Periodically retrain from scratch on a growing dataset of rollouts.

Because the learned layer is now a small macro-direction network, it should transfer more easily across goals than the previous per-candidate network.

## Future: supervised warm-start and self-play

The current single-phase REINFORCE training is simple and end-to-end. Future improvements may include:

1. **Supervised warm-start.** Train a value head on rollout completion times before policy-gradient training.
2. **Policy prior for UCT.** When full tree search is implemented, reuse the macro network output as the action prior inside UCB1.
3. **Self-play loop.** Run MCTS with the current network, store `(state, policy_target, value_target)` tuples, and retrain both heads.

These extensions reuse the same `MacroNet` architecture and state-featurization pipeline; they mainly change the loss function and data source.

## Epsilon decay

High-variance training runs often benefit from starting with more exploration and then gradually exploiting the learned policy. Set `epsilon` to the starting exploration probability, `epsilon_final` to the floor, and `epsilon_decay_episodes` to the number of episodes over which to decay:

```text
faf-sim train -e 2000 -m 10000 -r --epsilon 0.3 --epsilon-final 0.01 uef fatboy
```

The trainer uses linear decay:

```text
epsilon(ep) = epsilon - (epsilon - epsilon_final) * (ep / epsilon_decay_episodes)
```

Once the decay period ends, epsilon stays at `epsilon_final` for the rest of the run. The current epsilon is printed in the per-episode log column.

## When to stop

Stop iterating when:

- The trained policy reaches the goal consistently on the benchmark suite.
- Episode returns have plateaued for several training rounds.
- Adding more training data no longer improves completion times.

You can also set a concrete target completion time on the CLI. The trainer will keep running until that time is reached (or the episode budget is exhausted):

```text
faf-sim train -e 0 -m 10000 -t 20m -r uef fatboy
```

Here `-e 0` means "run forever", `-t 20m` stops the loop as soon as any episode finishes in 20 minutes or less, and `-r` resumes from the existing model. The best-seen model is saved automatically when training finishes.

At that point the system is ready for wider experimentation: harder goals, multiple goals, or integration into a larger bot.
