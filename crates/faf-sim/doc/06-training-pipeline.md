# 6. Training Pipeline

The MLP policy is trained with **REINFORCE**: roll out episodes with the current policy, then update the network so that actions leading to higher rewards become more likely.

## Single-phase policy-gradient training

Unlike a supervised warm-start that regresses on completion time, the current trainer learns directly from its own rollouts. The loop is:

1. Start a fresh `GraphState` with only the ACU.
2. Run the current policy for up to `max_steps` simulator ticks.
3. At each step, record the candidate feature matrix and the selected action index.
4. Compute a shaped reward from the final state.
5. Update the network with REINFORCE plus an entropy bonus.

This is implemented in `Trainer::train`:

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 139 — Trainer::train
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
// crates/faf-sim/src/planner/mcts/train.rs ~line 35 — TrainConfig
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
| `epsilon` | 0.1 | Initial probability of taking a random action. |
| `epsilon_final` | 0.1 | Final epsilon after decay. Same as `epsilon` when no decay. |
| `epsilon_decay_episodes` | same as `episodes` | Episodes over which to linearly decay `epsilon` to `epsilon_final`. `0` disables decay. |
| `entropy_coef` | 0.01 | Entropy bonus strength. |
| `target_time` | `None` | Stop early when an episode reaches the goal in at most this many seconds. |
| `verbose` | `false` | Print per-episode progress to stderr. |

The feature vector also includes storage-saturation signals from `PlannerConfig` (`max_mass_storage_count`, `max_energy_storage_count`), so saved model checkpoints from before storage support will fail to load and must be retrained.

## Rollout details

Each episode begins with a fresh state:

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 173 — Trainer::run_episode
let mut state = GraphState::new(units, &[UnitKind::Commander]);
```

At each tick:

1. Derive `SelectionPools` from the `PlanGraph`.
2. Build a feature matrix `[n_candidates, FEATURE_COUNT]`.
3. With probability `epsilon`, pick a random candidate; otherwise sample from the softmax over MLP scores.
4. Convert the chosen candidate to a `SimAction` and execute it.
5. If no candidate is executable, issue `Wait`.

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 188 — action selection
let pools = SelectionPools::new(plan, &state, units);
let candidates = pools.options().to_vec();

// ... feature matrix ...

let action_index = if self.rng.gen::<f32>() < self.config.epsilon {
    self.rng.gen_range(0..candidates.len())
} else {
    sample_action_index(&self.model, &candidate_features, &self.device, &mut self.rng)
};
```

## Reward shaping

Because the raw reward — reaching the goal — is sparse, the trainer uses a shaped reward computed from the final state of each episode:

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
// crates/faf-sim/src/planner/mcts/train.rs ~line 249 — Trainer::compute_returns
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

## Policy update

For each recorded step, the trainer:

1. Forwards the candidate feature matrix through the MLP.
2. Selects the log-probability of the action that was taken.
3. Multiplies by the normalized return.
4. Adds an entropy bonus to encourage exploration.
5. Backpropagates and applies Adam.

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 273 — Trainer::update
let log_probs = log_softmax(scores, 0);
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

## Future: supervised warm-start and self-play

The current single-phase REINFORCE training is simple and end-to-end. Future improvements may include:

1. **Supervised warm-start.** Train a value head on rollout completion times before policy-gradient training.
2. **Policy prior for UCT.** When full tree search is implemented, reuse the MLP output as the action prior inside UCB1.
3. **Self-play loop.** Run MCTS with the current network, store `(state, policy_target, value_target)` tuples, and retrain both heads.

These extensions reuse the same `ValueNet` architecture and feature pipeline; they mainly change the loss function and data source.

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
