# 5. Integration

This chapter explains how to wire the MCTS planner into the existing `faf-sim` planner enum and run it from the CLI (Command Line Interface).

## Strategy enum

The `Planner` dispatches to a single strategy, MCTS, but the network that guides it can be selected:

```rust
// crates/faf-sim/src/planner/core.rs ~line 67 — ValueNetKind
pub enum ValueNetKind {
    /// Multi-layer perceptron that scores candidates independently.
    #[default]
    Mlp,
    /// Graph neural network that reasons over the plan graph structure.
    Gnn,
}

// crates/faf-sim/src/planner/core.rs ~line 105 — Strategy enum
pub enum Strategy {
    /// Monte Carlo Tree Search guided by a learned value network.
    Mcts {
        /// Number of MCTS iterations to run per decision.
        iterations: usize,
        /// Kind of learned value network to use inside MCTS.
        value_net: ValueNetKind,
    },
}
```

Currently only `ValueNetKind::Mlp` is implemented; `Gnn` returns an error if selected.

## Entry point

`Planner::plan` matches on the strategy and forwards to the corresponding module:

```rust
// crates/faf-sim/src/planner/core.rs ~line 259 — Planner::plan dispatch
pub fn plan(
    &self,
    units: &Units,
    initial_state: GraphState,
    goal_id: &UnitKind,
) -> Result<PlanResult, PlannerError> {
    match self.strategy {
        Strategy::Mcts {
            iterations,
            value_net: value_net_kind,
        } => mcts::plan(
            units,
            initial_state,
            goal_id,
            iterations,
            value_net_kind,
            self.value_net.clone(),
            &self.config,
        ),
    }
}
```

The MCTS entry point is currently a stochastic one-step MLP policy:

```rust
// crates/faf-sim/src/planner/mcts/mod.rs ~line 45 — mcts::plan
pub fn plan(
    units: &Units,
    initial_state: GraphState,
    goal_id: &UnitKind,
    _iterations: usize,
    value_net_kind: ValueNetKind,
    value_net: Option<ValueNet<TrainBackend>>,
    config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    match value_net_kind {
        ValueNetKind::Mlp => mlp_policy_plan(units, initial_state, goal_id, value_net, config),
        ValueNetKind::Gnn => Err(PlannerError::UnsupportedStrategy(
            "GNN value net is not yet implemented".to_string(),
        )),
    }
}
```

It ignores the `iterations` parameter because there is no tree search yet. Full UCT search will replace the one-step policy while reusing the same `ValueNet`.

## Reactive planning

The MCTS planner is meant to run every tick. It does not commit to a full plan; it only returns the best immediate action. The surrounding actor executes that action, advances the simulator, and calls the planner again on the new state.

This closed-loop style protects against drift:

```text
loop:
    action = planner.plan(state, goal)
    state.apply(action)
    if goal_reached(state, goal): break
```

`PlanResult` already carries a `first_action` field for this purpose:

```rust
// crates/faf-sim/src/planner/core.rs ~line 19 — PlanResult (abbreviated)
pub struct PlanResult {
    pub events: Vec<BuildEvent>,
    pub completion_time: f64,
    pub final_economy: EconomyState,
    pub first_action: Option<crate::planner::search::SimAction>,
}
```

A reactive actor reads `first_action`, executes it, and ignores the projected `events` and `completion_time` because the real plan will be recomputed from the updated state.

## Actor wiring

The reactive loop is implemented as two Tokio actors that communicate over channels. The actor code lives in `crates/faf-sim/src/actors/`:

- `actors/sim_actor.rs` — owns the authoritative `GraphState` and ticks on a timer.
- `actors/decision_actor.rs` — owns the `Planner` and converts `first_action` into a `Command`.
- `actors/message.rs` — defines the `Command` and `Observation` messages exchanged between them.

The message protocol is intentionally small:

```rust
// crates/faf-sim/src/actors/message.rs ~line 15 — Command
pub enum Command {
    Build { unit_id: UnitKind, builder: NodeId },
    Assist { project_node: NodeId, builders: Vec<NodeId> },
    Upgrade { target_unit_id: UnitKind, old_node: NodeId, builder: NodeId },
}

// crates/faf-sim/src/actors/message.rs ~line 46 — Observation
pub enum Observation {
    Event(BuildEvent),
    State(GraphState),
}
```

`SimActor::run` advances the simulation and emits observations:

```rust
// crates/faf-sim/src/actors/sim_actor.rs ~line 73 — SimActor::run
pub async fn run(mut self) -> Result<GraphState, GraphSimError> {
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
// crates/faf-sim/src/actors/decision_actor.rs ~line 67 — DecisionActor::run
pub async fn run(mut self) {
    while let Some(observation) = self.obs_rx.recv().await {
        let command = match observation {
            Observation::State(state) => {
                let plan = self.planner.plan(&self.units, state, &self.goal_id).ok();
                plan.and_then(|p| p.first_action)
                    .and_then(sim_action_to_command)
            }
            Observation::Event(_) => None,
        };
        if let Some(command) = command {
            if self.cmd_tx.send(command).await.is_err() { break; }
        }
    }
}
```

For deterministic testing, `run_build_order_simulation` in `sim/runner.rs` pauses Tokio's clock, spawns both actors, and drives time forward in fixed increments:

```rust
// crates/faf-sim/src/sim/runner.rs ~line 113 — run_build_order_simulation
pub async fn run_build_order_simulation(
    units: Units,
    goal: UnitKind,
    config: SimulationConfig,
) -> Result<SimulationResult, SimulationError> {
    assert!(config.sim_dt > 0.0, "sim_dt must be positive");
    time::pause();

    let (obs_tx, obs_rx) = mpsc::channel::<Observation>(64);
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(64);

    let sim = SimActor::new(&[UnitKind::Commander], units.clone(),
                            Some(goal.clone()), config.sim_dt, obs_tx, cmd_rx);
    let sim_handle = tokio::spawn(sim.run());

    let decision_actor = DecisionActor::new(config.planner, units, goal, obs_rx, cmd_tx);
    let planner_handle = tokio::spawn(decision_actor.run());

    // ... drive timer until finished ...
    let final_state = sim_handle.await??;
    let _ = planner_handle.await;
    // ...
}
```

All three actor modules are re-exported from `crates/faf-sim/src/lib.rs` so callers can use `faf_sim::SimActor`, `faf_sim::DecisionActor`, `faf_sim::Command`, and `faf_sim::Observation` directly.

## CLI usage

The strategy can be parsed from a string:

```rust
// crates/faf-sim/src/planner/core.rs ~line 127 — Strategy::from_str MCTS parsing
if lower == "mcts" {
    return Ok(Strategy::Mcts {
        iterations: 100,
        value_net: ValueNetKind::Mlp,
    });
}

let Some(rest) = lower.strip_prefix("mcts") else {
    return Err(PlannerError::UnsupportedStrategy(s.to_string()));
};

let parts: Vec<&str> = rest.split(':').filter(|p| !p.is_empty()).collect();

match parts.len() {
    1 => {
        if let Ok(iterations) = parts[0].parse::<usize>() {
            Ok(Strategy::Mcts {
                iterations,
                value_net: ValueNetKind::Mlp,
            })
        } else {
            let value_net = ValueNetKind::from_str(parts[0])?;
            Ok(Strategy::Mcts {
                iterations: 100,
                value_net,
            })
        }
    }
    2 => {
        let iterations = parts[0]
            .parse::<usize>()
            .map_err(|_| PlannerError::UnsupportedStrategy(s.to_string()))?;
        let value_net = ValueNetKind::from_str(parts[1])?;
        Ok(Strategy::Mcts {
            iterations,
            value_net,
        })
    }
    _ => Err(PlannerError::UnsupportedStrategy(s.to_string())),
}
```

So the CLI can accept arguments such as:

```text
faf-sim simulate --strategy mcts cybran monkeylord
faf-sim simulate --strategy mcts:500 cybran monkeylord
faf-sim simulate --strategy mcts:500:gnn cybran monkeylord
faf-sim simulate --strategy mcts::gnn cybran monkeylord
```

When MCTS is stable, you can make it the default strategy for the CLI and benchmarks.

## Configuration

`PlannerConfig` is shared by all strategies. The current defaults are tuned for MCTS:

```rust
// crates/faf-sim/src/planner/core.rs ~line 198 — PlannerConfig default
fn default() -> Self {
    Self {
        dt: 1.0,
        max_depth: 400,
        max_mex_count: 8,
        max_pgen_count: 20,
    }
}
```

MCTS may benefit from a smaller `dt` because it explores fewer states than beam search and can afford finer-grained timing. Add MCTS-specific defaults in `Planner::new` if experiments show a better setting.

## Putting it together

A minimal integration test looks like this:

```rust
// docref: example
let planner = Planner::new(Strategy::Mcts {
    iterations: 100,
    value_net: ValueNetKind::Mlp,
});
let result = planner.plan(&units, initial_state, &UnitKind::Unique(UnitId("URL0402".to_string())))?;

if let Some(action) = result.first_action {
    println!("next action: {:?}", action);
}
```

## Training and model persistence

Train a model programmatically:

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 413 — train_policy
let (model, best_model, stats) = train_policy(&units, &goal, TrainConfig::default());
save_model(&model, &PathBuf::from("data/models/mlp-cybran-monkeylord")).unwrap();
```

And load it later:

```rust
// crates/faf-sim/src/planner/mcts/train.rs ~line 436 — load_model
let model = load_model(&PathBuf::from("data/models/mlp-cybran-monkeylord")).unwrap();
let planner = Planner::with_value_net(strategy, PlannerConfig::default(), model);
```

The CLI may wrap these calls with subcommands such as `train` and `simulate`; the programmatic API is the source of truth.
