# 6. Training with Online REINFORCE

This chapter is the core Burn/RL lesson. We take the policy network from [chapter 5](04-value-network.md), roll out episodes in the FAF simulator, and update the network weights with an **online REINFORCE policy-gradient update after every step**. There is no episode-level return computation, no return normalization, and no terminal/timeout penalty for the baseline.

## The training loop at a glance

The full training pipeline has three stages:

1. **Configuration** — `TrainConfig` holds hyperparameters.
2. **Episode generation** — `run_episode` rolls out one trajectory with the current policy, calling `update_step` after each action.
3. **Policy update** — `update_step` computes the masked log-probability of the selected direction, weights it by the step reward, and applies one gradient step immediately.

The whole loop is driven by `Trainer::train`. Because the plan graph is static for a given `Units` + `Goal`, `Trainer` builds it once on first use and reuses the same `Rc<PlanGraph>` for every episode.

## Configuration

`TrainConfig` groups the hyperparameters you will tune most often:

```rust
// crates/faf-sim/src/planner/policy/train/config.rs ~line 5 — TrainConfig
pub struct TrainConfig {
    pub episodes: usize,
    pub max_steps: usize,
    pub dt: f64,
    pub learning_rate: f64,
    pub gamma: f32,
    pub target_time: Option<f64>,
    pub grad_clip: Option<f32>,
}
```

Key knobs:

- `learning_rate` — Adam step size. Start around `1e-3`.
- `gamma` — kept in the config for compatibility, but the online baseline does **not** use discounting; each step is weighted by its own immediate reward.
- `grad_clip` — global gradient norm clipping. A value of `1.0` can stabilize REINFORCE early in training.

## Trainer setup

The `Trainer` owns the model, optimizer, device, and RNG. It is constructed with a fresh random model or from an existing one:

```rust
// crates/faf-sim/src/planner/policy/train/trainer/core.rs ~line 42 — Trainer::new
pub fn new(config: TrainConfig) -> Self {
    let device: TrainDevice = Default::default();
    let model = PolicyBundle::new(&device);
    Self::from_model(config, model)
}
```

```rust
// crates/faf-sim/src/planner/policy/train/trainer/core.rs ~line 49 — Trainer::from_model
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
// crates/faf-sim/src/planner/policy/train/trainer/core.rs ~line 19 — AdamOptimizer
pub type AdamOptimizer = OptimizerAdaptor<Adam, PolicyBundle<TrainBackend>, TrainBackend>;
```

## Episode generation

`run_episode` is the simulator loop. At each step it:

1. Featurizes the state.
2. Builds the legal-direction mask.
3. Selects the highest-probability legal direction (greedy argmax).
4. Resolves the direction to a concrete action via the heuristic layer.
5. Executes the action, computes the mass-income reward, and runs `update_step` immediately.

```rust
// crates/faf-sim/src/planner/policy/train/trainer/run_episode.rs ~line 20 — run_episode
pub(crate) fn run_episode(
    &mut self,
    units: &Units,
    goal: &Goal,
    planner_config: &PlannerConfig,
    plan: &PlanGraph,
) -> (Episode, f32) {
    let mut state = SimulationState::new(units, &[UnitKind::Commander]);
    let mut episode = Episode {
        reached_goal: false,
        completion_time: 0.0,
        steps: Vec::new(),
    };
    let mut accumulated_loss = 0.0f32;
    let mut step_count = 0usize;

    for _step in 0..self.config.max_steps {
        if state.goal_reached(goal) {
            episode.reached_goal = true;
            episode.completion_time = state.time;
            break;
        }

        let base_features = state_features(&state, units, planner_config);
        let direction_mask = legal_direction_mask(&state, units, planner_config, goal, plan);
        if direction_mask.iter().all(|&b| !b) {
            state.tick(units, self.config.dt);
            continue;
        }

        let direction_logits = self
            .model
            .evaluate_direction(base_features.clone(), &self.device);

        let direction_idx = masked_argmax(&direction_logits, &direction_mask).unwrap_or(0);
        let direction = EdgeCategory::ALL[direction_idx];

        let action = direction_to_action(direction, &state, units, planner_config, goal, plan);

        let prev_state = state.clone();
        if execute_action(&mut state, &action, units, self.config.dt).is_err() {
            state.tick(units, self.config.dt);
            continue;
        }

        let step_reward = compute_step_reward(&prev_state, &state, units, &self.config);
        let step = EpisodeStep {
            base_features,
            direction_mask,
            direction_index: direction_idx,
        };

        accumulated_loss += self.update_step(&step, step_reward);
        step_count += 1;
        episode.steps.push(step);
    }

    let avg_loss = if step_count == 0 {
        0.0
    } else {
        accumulated_loss / step_count as f32
    };
    (episode, avg_loss)
}
```

There are two important Burn patterns here:

- **Host-side selection.** `evaluate_direction` returns a plain `Vec<f32>` of logits. We build the mask and pick the highest-scoring legal direction on the CPU, then feed the selected index back into Burn as a tensor for the loss. This keeps the environment interaction on the host; only the gradient computation needs tensors.
- **Action masking.** `legal_direction_mask` tells us which of the six directions are currently feasible. Masking is applied twice: once during direction selection (so the policy only picks legal directions) and once during the loss (so gradients do not push toward illegal directions).

## The online REINFORCE update

The update step is where Burn really shines. For each recorded step we:

1. Build a `[1, 11]` input tensor from the stored features.
2. Run the backbone and direction head to get 6 logits.
3. Apply the legal-direction mask.
4. Compute `log_softmax` and extract the log-probability of the selected direction.
5. Weight the log-probability by the immediate step reward.
6. Backpropagate and apply one optimizer step right away.

```rust
// crates/faf-sim/src/planner/policy/train/trainer/update.rs ~line 20 — update_step
pub(crate) fn update_step(&mut self, step: &EpisodeStep, reward: f32) -> f32 {
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
    let direction_log_prob = direction_log_probs.select(0, direction_index_tensor);

    let reward_tensor = Tensor::<TrainBackend, 1>::from_data(
        TensorData::new(vec![reward], [1]),
        &self.device,
    );
    let loss = direction_log_prob.neg().mul(reward_tensor);
    let loss_value = loss.clone().into_data().as_slice::<f32>().unwrap()[0];

    let grads = loss.backward();
    let grads = burn::optim::GradientsParams::from_grads(grads, &self.model);
    self.model = self
        .optimizer
        .step(self.config.learning_rate, self.model.clone(), grads);

    loss_value
}
```

The policy-gradient objective for a single step is:

```text
loss = -log π(direction | state) * reward
```

- `direction_log_prob.neg().mul(reward_tensor)` implements `-log π * reward`. If the reward is positive, this term pushes the network to increase the probability of the selected direction; if negative, it pushes the network to decrease it.
- Because the update happens after every step, the gradient is local: it only tells the policy whether the chosen direction increased mass income right now, not whether the whole episode succeeded. This is the simplest possible credit assignment and the main limitation of the baseline.

## Masking inside the loss

Notice that the mask is applied inside the loss, not just during sampling. This is crucial: if we only masked during sampling, the loss would compare the network's unmasked distribution against a legal direction that the unmasked distribution might assign very low probability to. Masking inside the loss ensures the softmax is computed only over legal directions, so the gradient is well-behaved even when most directions are illegal.

## The main loop

`Trainer::train` orchestrates everything:

```rust
// crates/faf-sim/src/planner/policy/train/trainer/loop.rs ~line 19 — Trainer::train
pub fn train(&mut self, units: &Units, goal: &Goal) -> TrainStats {
    let planner_config = PlannerConfig {
        max_mex_count: self.config.max_mex_count,
        ..PlannerConfig::default()
    };
    if self.plan.is_none() {
        self.plan = Some(Rc::new(build_plan_graph(units, *goal)));
    }
    let plan = Rc::clone(self.plan.as_ref().expect("plan graph just initialized"));
    let mut stats = TrainStats::default();

    self.register_metrics();

    let mut ep = 0usize;

    loop {
        if self.should_stop_training(ep) {
            break;
        }

        let (episode, loss) = self.run_episode(units, goal, &planner_config, &plan);
        stats.episode_lengths.push(episode.steps.len());
        if !episode.steps.is_empty() {
            stats.losses.push(loss);
        }

        let target_hit = if episode.reached_goal {
            self.handle_goal_reached(&episode, &mut stats)
        } else {
            false
        };

        self.emit_episode_metrics(ep + 1, &episode, Some(loss));

        ep += 1;

        if target_hit {
            break;
        }
    }

    stats
}
```

The loop is deliberately simple: one episode, many gradient steps (one per successful action). `handle_goal_reached` tracks the fastest completion time for metrics, but it does **not** snapshot the model; the saved model is always the final parameters after training.

## Burn metrics

Training progress is reported through Burn's metric and renderer infrastructure. The trainer does not print directly; it emits typed events, a metric bundle turns those events into numeric state, and a pluggable renderer draws that state for the user.

### Data flow

The pipeline looks like this:

```text
Trainer::train
    └─ TrainEvent::Episode(EpisodeSummary { ... })
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
// crates/faf-sim/src/planner/policy/train/trainer/loop.rs ~line 91
self.emit_episode_metrics(ep + 1, &episode, Some(loss));
```

`FafSimMetrics` (in `crates/faf-sim/src/planner/policy/train/metric/metrics.rs`) owns one `Metric` implementation for each quantity we care about and forwards every event to all of them. Each metric decides whether the event is relevant; irrelevant events produce `"-"` so the renderer can skip them.

### Train events

There is one event variant:

- `TrainEvent::Episode` — emitted after every episode. It carries the average per-step loss for that episode.

### Metrics

`FafSimMetrics` registers the following metrics with the renderer:

| Metric | Source event | What it shows |
|---|---|---|
| `Episode Loss` | `Episode` | Average REINFORCE loss per successful step in the episode. |
| `Episode Steps` | `Episode` | Number of simulator steps in the episode. |
| `Completion Time` | `Episode` (goal reached) | Time in seconds when the goal was reached. |
| `Goal Reach` | `Episode` | `1.0` if the episode reached the goal, `0.0` otherwise. |
| `Best Time` | `Episode` (goal reached) | Fastest completion time observed so far across episodes that reached the goal. |
| `Episodes/sec` | `Episode` | Training throughput. |

All metrics are numeric and use Burn's `NumericMetricState`, so the renderer receives both a formatted string and a raw value for plotting.

### Renderers

The renderer is chosen by the CLI, not by the library. `faf-sim` library code only knows about `Box<dyn MetricsRenderer>`; the binary picks the concrete implementation.

#### TUI renderer (default)

By default `faf-sim-cli` opens the custom `train-tui` terminal dashboard when stdout is an interactive terminal. It follows Burn's layout conventions but removes the status panel and expands the metrics text panel:

```rust
// apps/faf-sim-cli/src/main.rs ~line 123
let use_tui = !args.quiet && !args.text && std::io::stdout().is_terminal();
```

```rust
// apps/faf-sim-cli/src/main.rs ~line 189
let renderer: Box<dyn MetricsRenderer> =
    if let Some(inter) = interrupter_for_renderer {
        Box::new(TrainTuiRenderer::new(inter))
    } else if quiet {
        Box::new(TextMetricsRenderer::quiet())
    } else {
        Box::new(TextMetricsRenderer::new())
    };
let metrics = FafSimMetrics::new(renderer);
```

The TUI renderer shows live plots for every registered metric. Press `q` to open the controls menu, then `s` to stop gracefully, `k` to kill training immediately, or `c`/`Esc` to cancel the menu.

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
- `reached` — whether the goal was reached.
- `time` — completion time if the goal was reached.
- `best` — best completion time seen so far.
- `loss` — average per-step episode loss.

#### Quiet renderer (`--quiet`)

`TextMetricsRenderer::quiet()` consumes metric updates but prints nothing. This is useful for CI or when piping output elsewhere.

### How to read the output

During normal training you should see:

- `loss` trending in magnitude as the policy learns which directions raise mass income.
- `reached` moving from mostly `false` to mostly `true` if the mass-income signal happens to pull the policy toward the goal path.
- `time` and `best` decreasing when goal-reaching episodes occur.

`best` is tracked only for diagnostics; the saved model is always the final parameters after training, not the model that produced the fastest training episode.

### Integration summary

- The trainer emits typed events.
- `FafSimMetrics` converts events into numeric metric state.
- A `MetricsRenderer` draws the state.
- `faf-sim-cli` chooses between the custom `train-tui` dashboard and a plain-text renderer based on CLI flags.
- The same metric code works for interactive TUI runs, log-friendly text runs, and silent `--quiet` runs.

## What you should remember

- **Host-side selection, tensor-side gradients.** The environment runs with Rust primitives; only the loss and backward pass use Burn tensors.
- **Mask twice.** Mask during action selection and again inside the loss.
- **Online REINFORCE objective.** `-log π * reward`, applied after every step.
- **One gradient step per action.** There is no episode-level accumulation or return standardization in the baseline.
- **Saved model is the final model.** We no longer keep a separate "best" model; whatever parameters exist after the last episode are saved.

With the policy trained, the next chapter shows how to wire it into the reactive simulator.
