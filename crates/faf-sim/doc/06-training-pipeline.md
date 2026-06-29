# 6. Training Pipeline

This chapter describes how the hierarchical policy bundle is trained. The pipeline uses REINFORCE with epsilon-greedy exploration and entropy regularization, periodically evaluates the policy greedily, and fine-tunes the best discovered trajectory at the end.

## Overview

The training entry point is `train_policy`:

```rust
// crates/faf-sim/src/planner/mcts/train/policy.rs ~line 60 — train_policy
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
    }
}
```

You can increase `episodes`, `max_steps`, and the network sizes for harder goals. The default is intended for quick experiments and unit tests.

## Trainer structure

The `Trainer` owns the model, optimizer, and running return statistics used to center the REINFORCE advantage:

```rust
// crates/faf-sim/src/planner/mcts/train/trainer.rs ~line 37 — Trainer (abbreviated)
pub struct Trainer {
    pub(crate) model: PolicyBundle<TrainBackend>,
    pub(crate) best_model: Option<PolicyBundle<TrainBackend>>,
    pub(crate) best_trajectory: Option<BuildTrajectory>,
    pub(crate) optimizer: AdamOptimizer,
    pub(crate) config: TrainConfig,
    pub(crate) device: TrainDevice,
    pub(crate) rng: ThreadRng,
    pub(crate) return_mean: f32,
    pub(crate) return_var: f32,
    pub(crate) return_count: f32,
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
    pub(crate) legal_mask: Vec<bool>,
    pub(crate) edge_index: usize,
    pub(crate) target_power: f32,
    pub(crate) desired_squad: [f32; 3],
    pub(crate) return_value: f32,
}
```

At each step the trainer:

1. Computes the legal edge mask.
2. Featurizes the state with shortfall feedback.
3. Samples an edge from the macro network (or a random legal edge with probability `epsilon`).
4. Samples target build power and engineer counts from the power and squad networks.
5. Resolves the squad into concrete builder nodes and executes the action.
6. Records the step.

If the episode exceeds `max_steps` without reaching the goal, it terminates.

## Reward signal

The reward is computed once per episode from the final state:

```rust
// crates/faf-sim/src/planner/mcts/train/reward.rs ~line 8 — compute_progress_reward
pub(crate) fn compute_progress_reward(
    state: &GraphState,
    units: &Units,
    goal: &UnitKind,
    plan: &PlanGraph,
) -> f32 {
    // reward for owning nodes on the plan graph, tech milestones,
    // economy scale, and a large bonus for reaching the goal quickly
}
```

The current reward combines:

- **Graph-node ownership.** A reward for every completed node on the plan graph, weighted inversely by its distance to the goal.
- **Tech milestones.** Bonuses for unlocking T1/T2/T3 factories and engineers.
- **Economy scale.** Clipped bonuses for total build power, mass income, and energy income.
- **Goal bonus.** A large positive reward when the goal is reached, plus a time premium for faster completion.
- **Failure penalty.** A fixed penalty if the episode does not reach the goal within the step budget.

The reward is dense enough to give a learning signal before the goal is reached, but the largest component is still the goal-completion bonus.

## REINFORCE update

After each episode, the trainer computes the discounted return for every timestep and subtracts a running-mean baseline to reduce variance. Then it updates all three networks jointly.

The combined loss has three parts:

1. **Macro loss.** Categorical log-likelihood of the selected edge, weighted by advantage, plus an entropy bonus.
2. **Build-power loss.** Gaussian log-likelihood of the sampled target power, weighted by advantage.
3. **Engineer-squad loss.** Gaussian log-likelihood of the sampled `[T1, T2, T3]` counts, weighted by advantage.

All three losses share the same advantage, so a single scalar drives the gradient through the macro net, the power net, and the squad net.

## Greedy evaluation

Every `greedy_eval_interval` episodes, the trainer runs a deterministic greedy rollout with the current parameters. If the greedy rollout reaches the goal faster than any previous greedy rollout, the current model is saved as `best_model`.

```rust
// crates/faf-sim/src/planner/mcts/train/trainer.rs ~line 158 — greedy evaluation
if interval > 0 && ep > 0 && (ep + 1) % interval == 0 {
    if let Some(greedy_time) =
        self.evaluate_greedy(units, goal, &plan, &edge_index, &planner_config)
    {
        let is_new_best = best_time.map_or(true, |t| greedy_time < t);
        if is_new_best {
            best_time = Some(greedy_time);
            self.best_model = Some(self.model.clone());
            self.best_trajectory = None;
        }
    }
}
```

Greedy evaluation is the source of the best model; REINFORCE alone does not guarantee that the final parameters are the best ones seen.

## Fine-tuning on the best trajectory

After the REINFORCE loop finishes, `fine_tune_best_model` runs supervised fine-tuning on the best trajectory discovered during training. If a best trajectory was recorded from an episode that set a new best time, the trainer creates a fresh optimizer around the best model and minimizes the same three losses with the recorded `(edge_index, target_power, desired_squad, shortfall)` targets.

```rust
// crates/faf-sim/src/planner/mcts/train/policy.rs ~line 94 — fine_tune_best_model
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

The function returns the fine-tuned model as both the final model and the best model. If no trajectory was recorded, it returns the final REINFORCE model and whatever `best_model` was stored.

## Saving and loading

Save the full bundle with `save_policy`:

```rust
// crates/faf-sim/src/planner/mcts/train/policy.rs ~line 13 — save_policy
pub fn save_policy(
    model: &PolicyBundle<TrainBackend>,
    path: &std::path::Path,
) -> Result<(), String> {
    // uses CompactRecorder
}
```

Load it with `load_policy`, passing the number of plan-graph edges so the macro network dimensions can be validated:

```rust
// crates/faf-sim/src/planner/mcts/train/policy.rs ~line 27 — load_policy
pub fn load_policy(
    path: &std::path::Path,
    num_edges: usize,
) -> Result<PolicyBundle<TrainBackend>, String> {
    // checks macro input/output dimensions and reports an error if mismatched
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
