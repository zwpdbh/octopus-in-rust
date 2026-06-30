# 6. Training with REINFORCE

This chapter describes how the hierarchical policy is trained. The pipeline uses REINFORCE with epsilon-greedy exploration and entropy regularization, periodically evaluates the policy greedily, and fine-tunes the best discovered trajectory at the end.

## Overview

The training entry point is `train_policy`:

```rust
// crates/faf-sim/src/planner/mcts/train/policy.rs ~line 51 — train_policy
pub fn train_policy(
    units: &Units,
    goal: &UnitKind,
    config: TrainConfig,
) -> (
    PolicyBundle<TrainBackend>,
    Option<PolicyBundle<TrainBackend>>,
    TrainStats,
) {
    let num_edges = plan_edge_index(units, goal)
        .expect("goal must have a plan graph")
        .len();
    let mut trainer = Trainer::new(config, num_edges);
    let stats = trainer.train(units, goal);
    fine_tune_best_model(trainer, units, goal, &config, stats)
}
```

It returns the final model, an optional best-seen model, and statistics. The optional best model comes from greedy evaluation: whenever the greedy rollout beats the previous best time, the current parameters are stored. After training, the best stored model is fine-tuned on the best trajectory and returned as the final model.

## Configuration

Training is controlled by `TrainConfig`:

```rust
// crates/faf-sim/src/planner/mcts/train/config.rs ~line 4 — TrainConfig
pub struct TrainConfig {
    /// Number of episodes to run.
    pub episodes: usize,
    /// Maximum simulator steps per episode.
    pub max_steps: usize,
    /// Fixed simulator timestep for rollouts.
    pub dt: f64,
    /// Learning rate for Adam.
    pub learning_rate: f64,
    /// Discount factor for future rewards.
    pub gamma: f32,
    /// Initial probability of taking a random action during training.
    pub epsilon: f32,
    /// Final epsilon value after decay.
    pub epsilon_final: f32,
    /// Number of episodes over which to linearly decay `epsilon`.
    pub epsilon_decay_episodes: usize,
    /// Entropy bonus coefficient.
    pub entropy_coef: f32,
    /// Stop early when the best completion time is at most this many seconds.
    pub target_time: Option<f64>,
    /// Evaluate greedily every N episodes and keep the best model.
    pub greedy_eval_interval: usize,
    /// Supervised fine-tuning epochs on the best trajectory.
    pub fine_tune_epochs: usize,
    /// Standard deviation for build-power sampling.
    pub power_std: f32,
    /// Standard deviation for engineer-count sampling.
    pub squad_std: f32,
    /// Print per-episode progress to stderr.
    pub verbose: bool,
    /// Stop early if no new best time for this many episodes.
    pub patience: Option<usize>,
}
```

The default configuration is conservative and CPU-friendly:

```rust
// crates/faf-sim/src/planner/mcts/train/config.rs ~line 44 — TrainConfig::default
fn default() -> Self {
    Self {
        episodes: 200,
        max_steps: 500,
        dt: 1.0,
        learning_rate: 1e-3,
        gamma: 0.99,
        epsilon: 0.1,
        epsilon_final: 0.1,
        epsilon_decay_episodes: 0,
        entropy_coef: 0.01,
        target_time: None,
        greedy_eval_interval: 100,
        fine_tune_epochs: 100,
        power_std: 2.0,
        squad_std: 0.5,
        verbose: false,
        patience: None,
    }
}
```

You can increase `episodes`, `max_steps`, and the network sizes for harder goals. The default is intended for quick experiments and unit tests.

## Trainer structure

The `Trainer` owns the model, optimizer, and best-seen state:

```rust
// crates/faf-sim/src/planner/mcts/train/trainer.rs ~line 40 — Trainer (abbreviated)
pub struct Trainer {
    pub(crate) model: PolicyBundle<TrainBackend>,
    pub(crate) best_model: Option<PolicyBundle<TrainBackend>>,
    pub(crate) best_trajectory: Option<BuildTrajectory>,
    pub(crate) optimizer: AdamOptimizer,
    pub(crate) config: TrainConfig,
    pub(crate) device: TrainDevice,
    pub(crate) rng: ThreadRng,
}
```

`Trainer::new` builds a fresh bundle and an Adam optimizer. `Trainer::from_model` continues from an existing bundle, which is useful for resuming training or fine-tuning.

## Episode generation

Each episode rolls out the current policy in the simulator. The trainer gathers a trajectory of recorded steps:

```rust
// crates/faf-sim/src/planner/mcts/train/episode.rs ~line 5 — EpisodeStep
pub(crate) struct EpisodeStep {
    pub(crate) base_features: Vec<f32>,
    pub(crate) shortfall: [f32; 3],
    pub(crate) direction_mask: Vec<bool>,
    pub(crate) action_mask: Vec<bool>,
    pub(crate) direction_index: usize,
    pub(crate) edge_index: usize,
    pub(crate) target_power: f32,
    pub(crate) desired_squad: [f32; 3],
    pub(crate) step_reward: f32,
    pub(crate) return_value: f32,
}
```

At each step the trainer:

1. Computes the legal direction mask and, for the chosen direction, the legal edge mask.
2. Featurizes the state with shortfall feedback.
3. Samples a direction from the direction head (or a random legal direction with probability `epsilon`).
4. Samples an edge from the action head for that direction (or a random legal edge with probability `epsilon`).
5. Samples target build power and engineer counts from the power and squad heads, adding Gaussian noise.
6. Resolves the squad into concrete builder nodes and executes the action.
7. Records the step, including the per-step reward.

If the episode exceeds `max_steps` without reaching the goal, it terminates.

## REINFORCE update

After each episode, the trainer calls `update` to perform one gradient step on all recorded steps. The combined loss has four parts:

1. **Direction loss.** Categorical log-likelihood of the sampled direction, weighted by advantage, plus an entropy bonus.
2. **Action loss.** Categorical log-likelihood of the sampled edge conditioned on the direction, weighted by advantage, plus an entropy bonus.
3. **Build-power loss.** Gaussian log-likelihood of the sampled target power, weighted by advantage.
4. **Engineer-squad loss.** Gaussian log-likelihood of the sampled `[T1, T2, T3]` counts, weighted by advantage.

All four losses share the same advantage, so a single scalar drives the gradient through every head.

```rust
// crates/faf-sim/src/planner/mcts/train/trainer.rs ~line 592 — update (abbreviated)
pub(crate) fn update(&mut self, episode: &Episode) -> f32 {
    for step in &episode.steps {
        // ... build macro input, run latent backbone once ...
        let latent = self.model.latent(macro_input);

        // Direction log-prob.
        let direction_logits = self.model.direction_logits(latent.clone()).flatten::<1>(0, 1);
        let masked_direction_logits = direction_logits + direction_mask_tensor;
        let direction_log_probs = log_softmax(masked_direction_logits, 0);
        let direction_log_prob = direction_log_probs.select(0, direction_index_tensor);

        // Action log-prob conditioned on the sampled direction.
        let action_logits = self
            .model
            .action_logits(latent.clone(), direction_one_hot)
            .flatten::<1>(0, 1);
        let masked_action_logits = action_logits + action_mask_tensor;
        let action_log_probs = log_softmax(masked_action_logits, 0);
        let action_log_prob = action_log_probs.select(0, edge_index_tensor);

        // Entropy bonus over both discrete distributions.
        let entropy = (direction_probs * direction_log_probs).neg().sum()
            + (action_probs * action_log_probs).neg().sum();

        // Continuous log-probs for power and squad.
        let power_log_prob = gaussian_log_prob_scalar(power_mean, step.target_power, ...);
        let squad_log_prob = gaussian_log_prob_vec(squad_means, &step.desired_squad, ...);

        let joint_log_prob = direction_log_prob + action_log_prob + power_log_prob + squad_log_prob;
        let policy_loss = joint_log_prob.neg().mul(return_tensor);
        let entropy_loss = entropy.neg().mul_scalar(self.config.entropy_coef);
        let loss = policy_loss + entropy_loss;

        // ... accumulate loss over the episode ...
    }

    let grads = loss.backward();
    let grads = burn::optim::GradientsParams::from_grads(grads, &self.model);
    self.model = self.optimizer
        .step(self.config.learning_rate.into(), self.model.clone(), grads);
}
```

This is the standard REINFORCE pattern, extended to a hierarchical policy. The discrete heads use `log_softmax` and `.select` to extract the log-probability of the sampled choice. The continuous heads use Gaussian log-probability helpers.

## Greedy evaluation

Every `greedy_eval_interval` episodes, the trainer runs a deterministic greedy rollout with the current parameters. If the greedy rollout reaches the goal faster than any previous greedy rollout, the current model is saved as `best_model`:

```rust
// crates/faf-sim/src/planner/mcts/train/trainer.rs ~line 445 — greedy evaluation
if interval > 0 && ep > 0 && (ep + 1) % interval == 0 {
    if let Some(greedy_time) =
        self.evaluate_greedy(units, goal, &plan, &edge_index, &planner_config)
    {
        let is_new_best = best_time.map_or(true, |t| greedy_time < t);
        if is_new_best {
            best_time = Some(greedy_time);
            episodes_since_best = 0;
            self.best_model = Some(self.model.clone());
            self.best_trajectory = None;
        }
    }
}
```

Greedy evaluation is the source of the best model; REINFORCE alone does not guarantee that the final parameters are the best ones seen.

## Fine-tuning on the best trajectory

After the REINFORCE loop finishes, `fine_tune_best_model` runs supervised fine-tuning on the best trajectory discovered during training. If a best trajectory was recorded from an episode that set a new best time, the trainer creates a fresh optimizer around the best model and minimizes cross-entropy/MSE losses with the recorded `(direction, edge_index, target_power, desired_squad, shortfall)` targets.

```rust
// crates/faf-sim/src/planner/mcts/train/policy.rs ~line 85 — fine_tune_best_model
fn fine_tune_best_model(
    mut trainer: Trainer,
    units: &Units,
    goal: &UnitKind,
    config: &TrainConfig,
    stats: TrainStats,
) -> (PolicyBundle<TrainBackend>, Option<PolicyBundle<TrainBackend>>, TrainStats) {
    // ... run fine_tune_epochs of supervised updates on the best trajectory ...
}
```

The function returns the fine-tuned model as the final model. If no trajectory was recorded, it returns the final REINFORCE model and whatever `best_model` was stored.

## Saving and loading

Save the full bundle with `save_policy`:

```rust
// crates/faf-sim/src/planner/mcts/train/policy.rs ~line 14 — save_policy
pub fn save_policy(
    model: &PolicyBundle<TrainBackend>,
    path: &std::path::Path,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("failed to create model dir: {e}"))?;
    }
    let recorder = CompactRecorder::new();
    recorder
        .record(model.clone().into_record(), path.to_path_buf())
        .map_err(|e| format!("failed to save model: {e}"))
}
```

Load it with `load_policy`, passing the number of plan-graph edges so the network dimensions can be validated:

```rust
// crates/faf-sim/src/planner/mcts/train/policy.rs ~line 28 — load_policy
pub fn load_policy(
    path: &std::path::Path,
    num_edges: usize,
) -> Result<PolicyBundle<TrainBackend>, String> {
    let device: TrainDevice = Default::default();
    let recorder = CompactRecorder::new();
    let record = recorder
        .load(path.to_path_buf(), &device)
        .map_err(|e| format!("failed to load model: {e}"))?;
    let model = PolicyBundle::new(&device, num_edges).load_record(record);

    if model.num_edges() != num_edges {
        return Err(format!(
            "action head output dimension mismatch: expected {num_edges}, got {}; retrain the model",
            model.num_edges()
        ));
    }

    Ok(model)
}
```

Old `.mpk` models saved before the hierarchical architecture change will not load; delete them and retrain.

## Monitoring progress

When `verbose` is enabled, the trainer prints one line per episode and progress during greedy evaluations:

```text
ep=   1 steps=  42 eps=0.1000 reached=false time=             - best=             - loss=    2.3456
ep= 100 steps=  38 eps=0.1000 reached= true time=      1243.50 best=      1243.50 loss=    1.8765
  greedy eval at ep=100: time=1234.00 best=1234.00
```

If progress stalls, try:

- Increasing `episodes` or `max_steps`.
- Raising or lowering `learning_rate`.
- Changing `gamma`.
- Adjusting `epsilon` or `entropy_coef`.
- Tuning the reward weights in `train::reward`.

## CLI training

The CLI wraps the programmatic API:

```text
faf-sim train -e 5000 -m 10000 uef novaxcenter
```

This trains a UEF `novaxcenter` bundle with 5000 episodes and up to 10000 steps per episode. Output is written to `data/models/mlp-uef-novaxcenter`.

By default the CLI prints a line after every episode and a progress line every few seconds during long episodes. Pass `--quiet` to suppress this output:

```text
faf-sim train -e 5000 -m 10000 --quiet uef novaxcenter
```

### Early stopping on a plateau

Set `--patience <N>` to stop training if the best completion time has not improved for `N` episodes. Patience is counted only **after the first successful episode**, so the run will keep trying until it finds at least one solution.

```text
faf-sim train -e 10000 -m 5000 --patience 1000 uef novaxcenter
```

This is useful for long training runs: instead of committing to a fixed episode budget, you let the trainer run until it stops making progress. You can combine it with `-t` (`target_time`) to stop as soon as a good enough time is reached.

### Disabling epsilon decay

By default epsilon decays from `--epsilon` to `--epsilon-final` over the run. If you want to keep exploring at a constant rate — for example when resuming from a saved model — pass `--no-epsilon-decay`. Epsilon then stays at the value of `--epsilon` for the whole run:

```text
# constant 10% random actions (default --epsilon)
faf-sim train -e 10000 -m 5000 --no-epsilon-decay uef novaxcenter

# constant 30% random actions
faf-sim train -e 10000 -m 5000 --epsilon 0.3 --no-epsilon-decay uef novaxcenter
```

## Testing

Unit tests for training live in `crates/faf-sim/src/planner/mcts/train/tests.rs`. They cover:

- Episode rollout shape and reward accumulation.
- `find_upgrade_source` helper.
- `assigned_squad_counts` helper.
- Tensor helper functions.
- Saving and loading a round-tripped bundle.

Run them with:

```text
cargo test -p faf-sim
```

There is no end-to-end integration test for a full training run because it is too slow for the normal test suite.
