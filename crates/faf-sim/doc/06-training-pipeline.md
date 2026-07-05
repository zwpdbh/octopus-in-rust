# 6. Training with REINFORCE

This chapter is the core Burn/RL lesson. We take the policy network from [chapter 5](04-value-network.md), roll out episodes in the FAF simulator, and update the network weights with the REINFORCE policy-gradient algorithm. We also cover entropy regularization, return normalization, greedy evaluation, and supervised fine-tuning on the best trajectory.

## The training loop at a glance

The full training pipeline has five stages:

1. **Configuration** — `TrainConfig` holds hyperparameters.
2. **Episode generation** — `run_episode` rolls out one trajectory with the current policy.
3. **Return computation** — `compute_returns` discounts and standardizes rewards.
4. **Policy update** — `update` computes the REINFORCE loss and applies one gradient step.
5. **Fine-tuning** — `fine_tune_on_trajectory` distills the best discovered trajectory into the final model.

The whole loop is driven by `Trainer::train`.

## Configuration

`TrainConfig` groups the hyperparameters you will tune most often:

```rust
// crates/faf-sim/src/planner/mcts/train/config.rs ~line 5 — TrainConfig
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
    pub greedy_eval_interval: usize,
    pub fine_tune_epochs: usize,
    pub grad_clip: Option<f32>,
}
```

Key knobs:

- `learning_rate` — Adam step size. Start around `1e-3`.
- `gamma` — discount factor for future rewards. We use `0.99`.
- `epsilon` / `epsilon_final` — probability of taking a random legal direction instead of sampling the policy. Decaying epsilon from `0.1` to `0.01` over a few hundred episodes is common.
- `entropy_coef` — entropy bonus coefficient. Higher values keep the policy exploratory for longer.
- `grad_clip` — global gradient norm clipping. A value of `1.0` can stabilize REINFORCE early in training.

## Trainer setup

The `Trainer` owns the model, optimizer, device, and RNG. It is constructed with a fresh random model or from an existing one:

```rust
// crates/faf-sim/src/planner/mcts/train/trainer/core.rs ~line 42 — Trainer::new
pub fn new(config: TrainConfig) -> Self {
    let device: TrainDevice = Default::default();
    let model = PolicyBundle::new(&device);
    Self::from_model(config, model)
}
```

```rust
// crates/faf-sim/src/planner/mcts/train/trainer/core.rs ~line 49 — Trainer::from_model
pub fn from_model(config: TrainConfig, model: PolicyBundle<TrainBackend>) -> Self {
    let device: TrainDevice = Default::default();
    let optimizer = {
        let adam = AdamConfig::new();
        let adam = if let Some(clip) = config.grad_clip {
            adam.with_grad_clipping(Some(GradientClippingConfig::Norm(clip)))
        } else {
            adam
        };
        adam.init()
    };
    // ... store model, optimizer, config, device, rng ...
}
```

Notice that Burn's optimizer is initialized with a model reference (`adam.init()`), but in our case we use the default `AdamConfig` and Burn infers parameter shapes later when we call `step`. The `OptimizerAdaptor` type alias ties the optimizer to the model type:

```rust
// crates/faf-sim/src/planner/mcts/train/trainer/core.rs ~line 19 — AdamOptimizer
pub type AdamOptimizer = OptimizerAdaptor<Adam, PolicyBundle<TrainBackend>, TrainBackend>;
```

## Episode generation

`run_episode` is the simulator loop. At each step it:

1. Featurizes the state.
2. Builds the legal-direction mask.
3. Samples a direction (epsilon-greedy over the masked softmax).
4. Resolves the direction to a concrete action via the heuristic layer.
5. Executes the action and records the reward.

```rust
// crates/faf-sim/src/planner/mcts/train/trainer/run_episode.rs ~line 21 — run_episode
pub(crate) fn run_episode(
    &mut self,
    _ep: usize,
    units: &Units,
    goal: &Goal,
    planner_config: &PlannerConfig,
    epsilon: f32,
) -> Episode {
    let plan = build_plan_graph(units, *goal);
    let mut state = SimulationState::new(units, &[UnitKind::Commander]);
    let mut episode = Episode { reached_goal: false, completion_time: 0.0, final_reward: 0.0, steps: Vec::new() };
    let mut milestones = MilestoneTracker::default();

    for _step in 0..self.config.max_steps {
        if state.goal_reached(goal) {
            episode.reached_goal = true;
            episode.completion_time = state.time;
            break;
        }

        let base_features = state_features(&state, units, planner_config);
        let direction_mask = legal_direction_mask(&state, units, planner_config, goal, &plan);
        if direction_mask.iter().all(|&b| !b) {
            state.tick(units, self.config.dt);
            continue;
        }

        let direction_logits = self
            .model
            .evaluate_direction(base_features.clone(), &self.device);

        let direction_idx = if self.rng.random::<f32>() < epsilon {
            let legal_directions: Vec<usize> = direction_mask
                .iter()
                .enumerate()
                .filter(|(_, &legal)| legal)
                .map(|(i, _)| i)
                .collect();
            *legal_directions
                .get(self.rng.random_range(0..legal_directions.len()))
                .unwrap_or(&0)
        } else {
            masked_sample_index(&direction_logits, &direction_mask, &mut self.rng).unwrap_or(0)
        };
        let direction = EdgeCategory::ALL[direction_idx];

        let action = direction_to_action(direction, &state, units, planner_config, goal, &plan);

        let prev_state = state.clone();
        if execute_action(&mut state, &action, units, self.config.dt).is_err() {
            state.tick(units, self.config.dt);
            continue;
        }

        let mut step_reward = compute_step_reward(&prev_state, &state, units);
        step_reward += milestones.update(&state, units);

        episode.steps.push(EpisodeStep {
            base_features,
            direction_mask,
            direction_index: direction_idx,
            step_reward,
            return_value: 0.0,
        });
    }

    episode.final_reward = compute_terminal_bonus(&state, episode.reached_goal);
    self.compute_returns(&mut episode);
    episode
}
```

There are two important Burn patterns here:

- **Host-side sampling.** `evaluate_direction` returns a plain `Vec<f32>` of logits. We build the mask and sample on the CPU, then feed the sampled index back into Burn as a tensor for the loss. This is common in RL: the environment interaction happens on the host; only the gradient computation needs tensors.
- **Action masking.** `legal_direction_mask` tells us which of the six directions are currently feasible. Masking is applied twice: once during sampling (so the policy only picks legal directions) and once during the loss (so gradients do not push toward illegal directions).

## Return computation

REINFORCE uses the discounted return from each step as the weight on its log-probability. `compute_returns` walks backward through the episode, applies the discount factor `gamma`, and then standardizes the returns:

```rust
// crates/faf-sim/src/planner/mcts/train/trainer/update.rs ~line 15 — compute_returns
pub(crate) fn compute_returns(&mut self, episode: &mut Episode) {
    let step_count = episode.steps.len();
    if step_count == 0 {
        return;
    }

    let gamma = self.config.gamma;
    let mut returns = Vec::with_capacity(step_count);
    let mut g = episode.final_reward;
    for step in episode.steps.iter().rev() {
        g = step.step_reward + gamma * g;
        returns.push(g);
    }
    returns.reverse();

    let mean = returns.iter().sum::<f32>() / step_count as f32;
    let std = (returns.iter().map(|r| (r - mean).powi(2)).sum::<f32>() / step_count as f32)
        .sqrt()
        .max(1e-6);

    for (step, ret) in episode.steps.iter_mut().zip(returns) {
        step.return_value = (ret - mean) / std;
    }
}
```

Standardization (subtract mean, divide by standard deviation) is a classic REINFORCE variance-reduction trick. It makes the optimizer less sensitive to the absolute reward scale and helps early training when returns can be noisy.

## The REINFORCE update

The update step is where Burn really shines. For each recorded step we:

1. Build a `[1, 11]` input tensor from the stored features.
2. Run the backbone and direction head to get 6 logits.
3. Apply the legal-direction mask.
4. Compute `log_softmax` and extract the log-probability of the selected direction.
5. Compute the entropy of the masked distribution.
6. Combine policy loss and entropy loss.
7. Accumulate losses across the episode and apply one gradient step.

```rust
// crates/faf-sim/src/planner/mcts/train/trainer/update.rs ~line 41 — update
pub(crate) fn update(&mut self, episode: &Episode) -> f32 {
    let mut accumulated_loss: Option<Tensor<TrainBackend, 1>> = None;
    let mut total_loss = 0.0f32;
    let mut step_count = 0usize;

    for step in &episode.steps {
        let features = step.base_features.clone();
        let macro_input = tensor1d_from_vec(&features);
        let latent = self.model.latent(macro_input);

        let direction_logits = self.model.direction_logits(latent).flatten::<1>(0, 1);
        let direction_mask: Vec<f32> = step
            .direction_mask
            .iter()
            .map(|&legal| if legal { 0.0 } else { MASK_VALUE })
            .collect();
        let direction_mask_tensor = Tensor::<TrainBackend, 1>::from_data(
            TensorData::new(direction_mask, [DIRECTION_COUNT]),
            &self.device,
        );
        let masked_direction_logits = direction_logits + direction_mask_tensor;
        let direction_log_probs = log_softmax(masked_direction_logits, 0);
        let direction_index_tensor = Tensor::<TrainBackend, 1, burn::tensor::Int>::from_data(
            TensorData::new(vec![step.direction_index as i64], [1]),
            &self.device,
        );
        let direction_log_prob = direction_log_probs
            .clone()
            .select(0, direction_index_tensor);

        let direction_probs = direction_log_probs.clone().exp();
        let direction_entropy = (direction_probs * direction_log_probs).neg().sum();

        let return_tensor = Tensor::<TrainBackend, 1>::from_data(
            TensorData::new(vec![step.return_value], [1]),
            &self.device,
        );
        let policy_loss = direction_log_prob.neg().mul(return_tensor);
        let entropy_loss = direction_entropy.neg().mul_scalar(self.config.entropy_coef);
        let loss = policy_loss + entropy_loss;

        total_loss += loss.clone().into_data().as_slice::<f32>().unwrap()[0];
        accumulated_loss = Some(match accumulated_loss {
            Some(acc) => acc + loss,
            None => loss,
        });
        step_count += 1;
    }

    if let Some(loss) = accumulated_loss {
        let grads = loss.backward();
        let grads = burn::optim::GradientsParams::from_grads(grads, &self.model);
        self.model = self
            .optimizer
            .step(self.config.learning_rate, self.model.clone(), grads);
    }

    total_loss / step_count.max(1) as f32
}
```

The policy-gradient objective for a single step is:

```text
loss = -log π(direction | state) * return - entropy_coef * entropy(π)
```

- `direction_log_prob.neg().mul(return_tensor)` implements `-log π * return`. If the return is positive, this term pushes the network to increase the probability of the selected direction; if negative, it pushes the network to decrease it.
- `entropy.neg().mul_scalar(entropy_coef)` is the entropy bonus. Entropy is computed from the masked distribution as `Σ -p * log p`. Maximizing entropy keeps the policy spread across legal directions and slows premature convergence.

Accumulating the loss over the whole episode and then calling `backward()` once is equivalent to averaging gradients over the episode. It is also efficient: Burn only builds one computation graph (albeit large) per update.

## Masking inside the loss

Notice that the mask is applied inside the loss, not just during sampling. This is crucial: if we only masked during sampling, the loss would compare the network's unmasked distribution against a legal direction that the unmasked distribution might assign very low probability to. Masking inside the loss ensures the softmax is computed only over legal directions, so the gradient is well-behaved even when most directions are illegal.

## Greedy evaluation

During training we periodically evaluate the current model greedily (argmax over masked logits). If the greedy rollout reaches the goal faster than any previous greedy rollout, we save that model as `best_model`:

```rust
// crates/faf-sim/src/planner/mcts/train/trainer/eval.rs ~line 33 — evaluate_greedy_with_model
pub(crate) fn evaluate_greedy_with_model(
    model: &PolicyBundle<TrainBackend>,
    units: &Units,
    goal: &Goal,
    planner_config: &PlannerConfig,
    max_steps: usize,
    dt: f64,
    device: &TrainDevice,
) -> Option<f64> {
    let plan = build_plan_graph(units, *goal);
    let mut state = SimulationState::new(units, &[UnitKind::Commander]);

    for _ in 0..max_steps {
        if state.goal_reached(goal) {
            return Some(state.time);
        }

        let features = state_features(&state, units, planner_config);
        let direction_logits = model.evaluate_direction(features, device);
        let direction_mask = legal_direction_mask(&state, units, planner_config, goal, &plan);

        if direction_mask.iter().all(|&b| !b) {
            state.tick(units, dt);
            continue;
        }

        let direction_idx = masked_argmax(&direction_logits, &direction_mask).unwrap_or(0);
        let direction = EdgeCategory::ALL[direction_idx];
        let action = direction_to_action(direction, &state, units, planner_config, goal, &plan);

        if execute_action(&mut state, &action, units, dt).is_err() {
            state.tick(units, dt);
        }
    }

    None
}
```

Greedy evaluation is the source of the best model. REINFORCE alone does not guarantee that the final parameters are the best ones seen.

## Fine-tuning on the best trajectory

After REINFORCE finishes, the trainer runs supervised fine-tuning on the best trajectory discovered during training. This is a Burn cross-entropy (negative log-likelihood) loss that distills the best episode into the model:

```rust
// crates/faf-sim/src/planner/mcts/train/trainer/fine_tune.rs ~line 23 — fine_tune_on_trajectory
pub(crate) fn fine_tune_on_trajectory(
    &mut self,
    trajectory: &BuildTrajectory,
    units: &Units,
    goal: &Goal,
    planner_config: &PlannerConfig,
) -> f32 {
    if trajectory.steps.is_empty() {
        return 0.0;
    }

    let plan = build_plan_graph(units, *goal);
    let mut state = SimulationState::new(units, &[UnitKind::Commander]);
    let mut accumulated_loss: Option<Tensor<TrainBackend, 1>> = None;
    let mut total_loss_value = 0.0f32;
    let mut step_count = 0usize;

    for step in &trajectory.steps {
        let mut executable = false;
        for _ in 0..self.config.max_steps {
            let direction_mask =
                legal_direction_mask(&state, units, planner_config, goal, &plan);
            if !direction_mask[step.direction_index] {
                state.tick(units, planner_config.dt);
                continue;
            }

            let base_features = state_features(&state, units, planner_config);
            let macro_input = tensor1d_from_vec(&base_features);
            let latent = self.model.latent(macro_input);

            let direction_logits = self.model.direction_logits(latent).flatten::<1>(0, 1);
            let direction_mask_tensor = Tensor::<TrainBackend, 1>::from_data(
                TensorData::new(
                    direction_mask
                        .iter()
                        .map(|&legal| if legal { 0.0 } else { MASK_VALUE })
                        .collect(),
                    [DIRECTION_COUNT],
                ),
                &self.device,
            );
            let masked_direction_logits = direction_logits + direction_mask_tensor;
            let direction_log_probs = log_softmax(masked_direction_logits, 0);
            let direction_index_tensor =
                Tensor::<TrainBackend, 1, burn::tensor::Int>::from_data(
                    TensorData::new(vec![step.direction_index as i64], [1]),
                    &self.device,
                );
            let direction_ce = direction_log_probs.select(0, direction_index_tensor).neg();

            total_loss_value += direction_ce.clone().into_data().as_slice::<f32>().unwrap()[0];
            accumulated_loss = Some(match accumulated_loss {
                Some(acc) => acc + direction_ce,
                None => direction_ce,
            });
            step_count += 1;

            let direction = EdgeCategory::ALL[step.direction_index];
            let action =
                direction_to_action(direction, &state, units, planner_config, goal, &plan);
            let _ = execute_action(&mut state, &action, units, planner_config.dt);
            executable = true;
            break;
        }

        if !executable {
            break;
        }
    }

    if let Some(loss) = accumulated_loss {
        let grads = loss.backward();
        let grads = burn::optim::GradientsParams::from_grads(grads, &self.model);
        self.model = self
            .optimizer
            .step(self.config.learning_rate, self.model.clone(), grads);
    }

    total_loss_value / step_count.max(1) as f32
}
```

Fine-tuning is supervised because we already know the direction that was taken in the best trajectory. We simply maximize the log-probability of those directions under the current model. This can clean up the policy after noisy REINFORCE exploration.

## The main loop

`Trainer::train` orchestrates everything:

```rust
// crates/faf-sim/src/planner/mcts/train/trainer/loop.rs ~line 16 — Trainer::train
pub fn train(&mut self, units: &Units, goal: &Goal) -> TrainStats {
    let planner_config = PlannerConfig::default();
    let mut stats = TrainStats::default();

    // ... optional greedy eval of best_model if resuming ...

    loop {
        // stop on episode limit or user interrupt
        let epsilon = self.current_epsilon(ep);
        let episode = self.run_episode(units, goal, &planner_config, epsilon, &plan);

        let loss = if !episode.steps.is_empty() {
            let loss = self.update(&episode);
            stats.losses.push(loss);
            Some(loss)
        } else {
            None
        };

        if episode.reached_goal {
            // track best time, save best_model and best_trajectory
        }

        // periodic greedy evaluation
        if interval > 0 && ep > 0 && (ep + 1).is_multiple_of(interval) {
            if let Some(greedy_time) = self.evaluate_greedy(units, goal, &planner_config) {
                // update best_time / best_model
            }
        }

        // emit Burn metrics
        // ...
    }

    stats
}
```

The loop is deliberately simple: one episode, one gradient step. This makes it easy to reason about and easy to extend with more sophisticated algorithms later.

## Burn metrics

Training progress is reported through Burn's metric and renderer infrastructure. The trainer does not print directly; it emits typed events, a metric bundle turns those events into numeric state, and a pluggable renderer draws that state for the user.

### Data flow

The pipeline looks like this:

```text
Trainer::train
    ├─ TrainEvent::Episode(EpisodeSummary { ... })
    └─ TrainEvent::GreedyEval(GreedyEvalSummary { ... })
           ↓
       FafSimMetrics::update(event, metadata)
           ↓
       per-metric NumericMetricState
           ↓
       MetricsRenderer::update_train(state)
           ↓
       TUI dashboard or plain-text lines
```

In `Trainer::train`:

```rust
// crates/faf-sim/src/planner/mcts/train/trainer/loop.rs ~line 137
if let Some(ref mut metrics) = self.metrics {
    metrics.update(
        &TrainEvent::Episode(EpisodeSummary {
            episode: ep + 1,
            total_episodes: self.config.episodes,
            steps: episode.steps.len(),
            epsilon,
            reached_goal: episode.reached_goal,
            completion_time: episode.completion_time,
            loss,
            best_time,
        }),
        &metadata,
    );
    metrics.render(
        training_progress(ep + 1, self.config.episodes, Some(ep + 1)),
        vec![],
    );
}
```

`FafSimMetrics` (in `crates/faf-sim/src/planner/mcts/train/metric/metrics.rs`) owns one `Metric` implementation for each quantity we care about and forwards every event to all of them. Each metric decides whether the event is relevant; irrelevant events produce `"-"` so the renderer can skip them.

### Train events

There are three event variants:

- `TrainEvent::Episode` — emitted after every REINFORCE episode.
- `TrainEvent::GreedyEval` — emitted after each periodic greedy evaluation.
- `TrainEvent::FineTuneEpoch` — emitted during supervised fine-tuning on the best trajectory.

### Metrics

`FafSimMetrics` registers the following metrics with the renderer:

| Metric | Source event | What it shows |
|---|---|---|
| `Episode Loss` | `Episode` | Average REINFORCE loss for the episode. |
| `Episode Steps` | `Episode` | Number of simulator steps in the episode. |
| `Completion Time` | `Episode` (goal reached) | Time in seconds when the goal was reached. |
| `Goal Reach` | `Episode` | `1.0` if the episode reached the goal, `0.0` otherwise. |
| `Epsilon` | `Episode` | Current epsilon-greedy exploration probability. |
| `Best Time` | `Episode` or `GreedyEval` | Fastest completion time observed so far. |
| `Greedy Eval Time` | `GreedyEval` (goal reached) | Completion time of the latest greedy rollout. |
| `Episodes/sec` | `Episode` | Training throughput. |
| `Fine-Tune Loss` | `FineTuneEpoch` | Supervised cross-entropy loss on the best trajectory. |

All metrics are numeric and use Burn's `NumericMetricState`, so the renderer receives both a formatted string and a raw value for plotting.

### Renderers

The renderer is chosen by the CLI, not by the library. `faf-sim` library code only knows about `Box<dyn MetricsRenderer>`; the binary picks the concrete implementation.

#### TUI renderer (default)

By default `faf-sim-cli` opens Burn's built-in terminal dashboard when stdout is an interactive terminal:

```rust
// apps/faf-sim-cli/src/main.rs ~line 116
let use_tui = !args.quiet && !args.text && std::io::stdout().is_terminal();
```

```rust
// apps/faf-sim-cli/src/main.rs ~line 177
let renderer: Box<dyn MetricsRenderer> =
    if let Some(inter) = interrupter_for_renderer {
        Box::new(TuiMetricsRendererWrapper::new(inter, None))
    } else if quiet {
        Box::new(TextMetricsRenderer::quiet())
    } else {
        Box::new(TextMetricsRenderer::new())
    };
let metrics = FafSimMetrics::new(renderer);
```

The TUI renderer shows live plots for every registered metric. It also accepts an `Interrupter`, so pressing the stop key in the dashboard gracefully ends training at the next episode boundary.

#### Text renderer (`--text`)

Pass `--text` to disable the dashboard and use `TextMetricsRenderer` (`apps/faf-sim-cli/src/text_renderer.rs`) instead. It prints one line per episode to stderr:

```text
ep=   1 steps=  42 eps=0.1000 reached=false time=             - best=             - loss=    1.2345
ep=  10 steps=  38 eps=0.0955 reached= true time=      2m 15.3s best=      2m 15.3s loss=    0.9876
ep=  50 steps=  31 eps=0.0782 reached= true time=      1m 52.1s best=      1m 48.7s loss=    0.5432
```

Columns:

- `ep` — episode number.
- `steps` — simulator steps taken.
- `eps` — current epsilon (exploration probability).
- `reached` — whether the goal was reached.
- `time` — completion time if the goal was reached.
- `best` — best completion time seen so far.
- `loss` — average episode loss.

#### Quiet renderer (`--quiet`)

`TextMetricsRenderer::quiet()` consumes metric updates but prints nothing. This is useful for CI or when piping output elsewhere.

### How to read the output

During normal training you should see:

- `loss` trending downward as the policy learns.
- `reached` moving from mostly `false` to mostly `true`.
- `time` and `best` decreasing as the policy finds faster build orders.
- `eps` decaying from `epsilon` toward `epsilon_final`.

`best` is the most important column for model selection. REINFORCE does not guarantee monotonic improvement, so `Trainer::train` keeps the model that produced the fastest greedy rollout, not necessarily the final parameters. The TUI plots `Best Time` and `Greedy Eval Time` so you can watch this directly.

### Integration summary

- The trainer emits typed events.
- `FafSimMetrics` converts events into numeric metric state.
- A `MetricsRenderer` draws the state.
- `faf-sim-cli` chooses between Burn's TUI renderer and a plain-text renderer based on CLI flags.
- The same metric code works for interactive TUI runs, log-friendly text runs, and silent `--quiet` runs.

## What you should remember

- **Host-side sampling, tensor-side gradients.** The environment runs with Rust primitives; only the loss and backward pass use Burn tensors.
- **Mask twice.** Mask during action selection and again inside the loss.
- **REINFORCE objective.** `-log π * return - entropy_coef * entropy`.
- **One gradient step per episode.** Accumulate per-step losses, then `backward()` + `optimizer.step()`.
- **Greedy eval chooses the best model.** Track the fastest greedy completion time and keep that model.
- **Fine-tuning distills the best trajectory.** Cross-entropy on the recorded directions can polish the final policy.

With the policy trained, the next chapter shows how to use it as a prior inside MCTS.
