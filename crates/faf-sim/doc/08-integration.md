# 8. Integration and CLI

This chapter explains how the learned policy planner is wired into the `faf-sim` planner enum and run from the CLI.

## Strategy enum

The `Planner` dispatches to a single strategy, `Policy`, but the network architecture can be selected:

```rust
// crates/faf-sim/src/planner/core.rs ~line 110 — ValueNetKind
pub enum ValueNetKind {
    /// Hierarchical policy bundle.
    #[default]
    Mlp,
    /// Graph neural network that reasons over the plan graph structure.
    Gnn,
}

// crates/faf-sim/src/planner/core.rs ~line 148 — Strategy enum
pub enum Strategy {
    /// Direct one-step policy lookup guided by a learned network.
    Policy {
        /// Kind of learned policy network to use.
        value_net: ValueNetKind,
        /// If true, always pick the highest-scoring legal direction.
        deterministic: bool,
    },
}
```

Currently only `ValueNetKind::Mlp` is implemented; `Gnn` returns an error if selected. `Mlp` refers to the direction-only policy bundle (backbone + direction head).

## Entry point

`Planner::plan` evaluates the policy once, masks illegal directions, and returns the best immediate action:

```rust
// crates/faf-sim/src/planner/core.rs ~line 318 — Planner::plan
pub fn plan(
    &mut self,
    units: &Units,
    initial_state: SimulationState,
    goal: &Goal,
) -> Result<PlanResult, PlannerError> {
    let Strategy::Policy { value_net, deterministic } = self.strategy;
    direction_planner::plan(
        units,
        initial_state,
        goal,
        1,
        value_net,
        deterministic,
        Some(self.value_net.as_ref()),
        &self.config,
    )
}
```

The one-step direction-only policy lives in `policy::direction_planner::plan`, which is the same entry point used by training rollouts and the reactive simulator. The concrete network is hidden behind a [`ValueNet`] trait object.

## Reactive planning

The policy planner is meant to run every tick. It does not commit to a full plan; it only returns the best immediate action. The surrounding actor executes that action, advances the simulator, and calls the planner again on the new state.

This closed-loop style protects against drift:

```text
loop:
    action = planner.plan(state, goal)
    state.apply(action)
    if goal_reached(state, goal): break
```

`PlanResult` carries a `first_action` field for this purpose:

```rust
// crates/faf-sim/src/planner/core.rs ~line 55 — PlanResult (abbreviated)
pub struct PlanResult {
    pub events: Vec<BuildEvent>,
    pub completion_time: f64,
    pub final_economy: EconomyState,
    pub first_action: Option<crate::planner::SimAction>,
    pub final_state: SimulationState,
}
```

A reactive actor reads `first_action`, executes it, and ignores the projected `events` and `completion_time` because the real plan will be recomputed from the updated state.

## Actor wiring

The reactive loop is implemented as two Tokio actors that communicate over channels. The actor code lives in `crates/faf-sim/src/actors/`:

- `actors/sim_actor.rs` — owns the authoritative `SimulationState` and ticks on a timer.
- `actors/decision_actor.rs` — owns the `Planner` and converts `first_action` into a `SimulationMsg`.
- `actors/message.rs` — defines the `SimulationMsg` and `Observation` messages exchanged between them.

The message protocol is intentionally small:

```rust
// crates/faf-sim/src/actors/message.rs ~line 16 — SimulationMsg
pub enum SimulationMsg {
    Build { unit_id: UnitKind, builders: Vec<NodeId> },
    Assist { project_node: NodeId, builders: Vec<NodeId> },
    Upgrade { target_unit_id: UnitKind, old_node: NodeId, builders: Vec<NodeId> },
    BuildGoal { goal: Goal, builders: Vec<NodeId> },
}

// crates/faf-sim/src/actors/message.rs ~line 54 — Observation
pub enum Observation {
    Event(BuildEvent),
    State(SimulationState),
}
```

`Build`, `Upgrade`, and `BuildGoal` carry a `Vec<NodeId>` so the planner can assign a squad of engineers to a project immediately, rather than only one builder.

`SimActor::run` advances the simulation and emits observations:

```rust
// crates/faf-sim/src/actors/sim_actor.rs ~line 74 — SimActor::run
pub async fn run(mut self) -> Result<SimulationState, GraphSimError> {
    loop {
        tokio::select! {
            _ = self.timer.tick() => {
                self.tick_and_report().await?;
                if self.goal_reached() { break; }
            }
            maybe_cmd = self.cmd_rx.recv() => {
                match maybe_cmd {
                    Some(cmd) => self.apply_command(cmd)?,
                    None => break,
                }
            }
        }
    }
    Ok(self.state)
}
```

`DecisionActor::run` waits for state observations, calls `Planner::plan`, and sends the resulting command back:

```rust
// crates/faf-sim/src/actors/decision_actor.rs ~line 69 — DecisionActor::run
pub async fn run(mut self) {
    while let Some(observation) = self.obs_rx.recv().await {
        match observation {
            Observation::State(state) => {
                let plan = match self.planner.plan(&self.units, state, &self.goal) {
                    Ok(p) => p,
                    Err(_) => continue,
                };

                let Some(command) = plan.first_action.and_then(sim_action_to_command) else {
                    continue;
                };

                if self.cmd_tx.send(command).await.is_err() {
                    return;
                }
            }
            Observation::Event(_) => {}
        }
    }
}
```

For deterministic testing, `run_build_order_simulation` in `sim/runner.rs` pauses Tokio's clock, spawns both actors, and drives time forward in fixed increments:

```rust
// crates/faf-sim/src/sim/runner.rs ~line 117 — run_build_order_simulation
pub async fn run_build_order_simulation(
    units: Units,
    goal: Goal,
    config: SimulationConfig,
) -> Result<SimulationResult, SimulationError> {
    assert!(config.sim_dt > 0.0, "sim_dt must be positive");
    time::pause();

    let (obs_tx, obs_rx) = mpsc::channel::<Observation>(64);
    let (cmd_tx, cmd_rx) = mpsc::channel::<SimulationMsg>(64);

    let sim = SimActor::new(
        &[UnitKind::Commander],
        units.clone(),
        Some(goal),
        config.sim_dt,
        obs_tx,
        cmd_rx,
    );
    let sim_handle = tokio::spawn(sim.run());

    let decision_actor = DecisionActor::new(config.planner, units, goal, obs_rx, cmd_tx);
    let planner_handle = tokio::spawn(decision_actor.run());

    // ... drive timer until finished ...
    let final_state = sim_handle.await??;
    let _ = planner_handle.await;
    // ...
}
```

All three actor modules are re-exported from `crates/faf-sim/src/lib.rs` so callers can use `faf_sim::SimActor`, `faf_sim::DecisionActor`, `faf_sim::SimulationMsg`, and `faf_sim::Observation` directly.

## CLI usage

The strategy can be parsed from a string:

```rust
// crates/faf-sim/src/planner/core.rs ~line 211 — Strategy::from_str policy parsing
if lower == "policy" {
    return Ok(Strategy::Policy {
        value_net: ValueNetKind::Mlp,
        deterministic: false,
    });
}

if let Some(rest) = lower.strip_prefix("policy") {
    let parts: Vec<&str> = rest.split(':').filter(|p| !p.is_empty()).collect();
    let mut value_net = ValueNetKind::Mlp;
    let mut deterministic = false;
    for part in parts {
        if part == "greedy" || part == "deterministic" {
            deterministic = true;
        } else {
            value_net = ValueNetKind::from_str(part)?;
        }
    }
    return Ok(Strategy::Policy { value_net, deterministic });
}
```

So the CLI can accept arguments such as:

```text
faf-sim simulate --strategy policy cybran monkeylord
faf-sim simulate --strategy policy:mlp:greedy cybran monkeylord
faf-sim simulate --strategy policy:gnn:greedy cybran monkeylord
```

The default strategy for `simulate` is `policy:mlp:greedy`.

### GPU builds

The `faf-sim-cli` package defaults to CUDA, so no extra feature flags are needed for NVIDIA GPUs:

```text
# NVIDIA GPU (your 3090) — default
cargo run --release -p faf-sim-cli -- simulate --strategy policy:mlp:greedy uef novaxcenter

# Cross-platform WebGPU/Vulkan
cargo run --release -p faf-sim-cli --no-default-features --features wgpu -- simulate --strategy policy:mlp:greedy uef novaxcenter

# CPU-only fallback
cargo run --release -p faf-sim-cli --no-default-features --features cpu -- simulate --strategy policy:mlp:greedy uef novaxcenter
```

The same default applies to the `train` subcommand.

## Configuration

`PlannerConfig` is shared by planning and training. The current defaults are tuned for the reactive policy loop:

```rust
// crates/faf-sim/src/planner/core.rs ~line 260 — PlannerConfig default
fn default() -> Self {
    Self {
        dt: 1.0,
        max_depth: 400,
        max_mex_count: 12,
    }
}
```

## Putting it together

A minimal integration test looks like this:

```rust
// docref: example
use faf_sim::planner::policy::value_net::MlpValueNet;

let goal = Goal {
    tech_level: TechLevel::T4,
    mass_cost: 28_000.0,
    energy_cost: 340_000.0,
    build_time: 46_250.0,
};
let value_net = Box::new(MlpValueNet::from_net(bundle));
let planner = Planner::with_config(
    Strategy::Policy {
        value_net: ValueNetKind::Mlp,
        deterministic: true,
    },
    PlannerConfig::default(),
    value_net,
);
let result = planner.plan(&units, initial_state, &goal)?;

if let Some(action) = result.first_action {
    println!("next action: {:?}", action);
}
```

## Training and model persistence

Train a policy bundle programmatically:

```rust
// docref: example
use faf_sim::planner::policy::train::{train_policy, save_policy, FafSimMetrics, TrainConfig};
use burn::train::renderer::tui::TuiMetricsRendererWrapper;
use burn::train::Interrupter;

let goal = Goal {
    tech_level: TechLevel::T4,
    mass_cost: 28_000.0,
    energy_cost: 340_000.0,
    build_time: 46_250.0,
};
let metrics = FafSimMetrics::new(Box::new(TuiMetricsRendererWrapper::new(
    Interrupter::new(),
    None,
)));
let (bundle, best_bundle, stats) = train_policy(
    &units,
    &goal,
    TrainConfig::default(),
    metrics,
    None,
    Interrupter::new(),
);
save_policy(
    best_bundle.as_ref().unwrap_or(&bundle),
    &PathBuf::from("data/models/mlp-cybran-monkeylord"),
)
.unwrap();
```

And load it later:

```rust
// docref: example
use faf_sim::planner::policy::value_net::MlpValueNet;
use faf_sim::planner::policy::train::load_policy;

let bundle = load_policy(&PathBuf::from("data/models/mlp-cybran-monkeylord")).unwrap();
let value_net = Box::new(MlpValueNet::from_net(bundle));
let planner = Planner::with_config(strategy, PlannerConfig::default(), value_net);
// `goal` is the same abstract Goal used during training.
```

The CLI wraps these calls with `train` and `simulate` subcommands. The programmatic API is the source of truth.

## Terminal dashboard

For interactive training, the `faf-sim-cli` binary uses Burn's built-in terminal UI renderer (`TuiMetricsRendererWrapper`) when stdout is a terminal. The renderer is created inside the training thread and shows the standard Burn metric dashboard.

```rust
// apps/faf-sim-cli/src/main.rs ~line 177 — run_train renderer setup
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

Pass `--text` to keep plain-text output, or `--quiet` to suppress live progress entirely. Inside the Burn TUI, use the renderer's normal quit key to stop training gracefully at the next episode boundary.

## Model compatibility

Saved policy bundles from before the direction-only refactor will fail to load and must be retrained. The policy network now consumes 11 state features and outputs 6 direction logits; `load_policy` creates a model of that shape and loads the record into it, and a shape mismatch produces a clear deserialization error.
