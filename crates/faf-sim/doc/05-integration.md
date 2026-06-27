# 5. Integration

This chapter explains how to wire the MCTS planner into the existing `faf-sim` planner enum and run it from the CLI (Command Line Interface).

## Strategy enum

The `Planner` dispatches to a single strategy, MCTS, but the value network that guides it can be selected:

```rust
// crates/faf-sim/src/planner/core.rs ~line 75 — Strategy enum
pub enum ValueNetKind {
    Mlp,
    Gnn,
}

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

The MCTS variant is already declared. The missing piece is the implementation of `mcts::plan`.

## Entry point

`Planner::plan` matches on the strategy and forwards to the corresponding module:

```rust
// crates/faf-sim/src/planner/core.rs ~line 196 — Planner::plan dispatch
pub fn plan(
    &self,
    units: &Units,
    initial_state: GraphState,
    goal_id: &UnitKind,
) -> Result<PlanResult, PlannerError> {
    match self.strategy {
        Strategy::Mcts {
            iterations,
            value_net,
        } => mcts::plan(units, initial_state, goal_id, iterations, value_net, &self.config),
    }
}
```

The MCTS entry point is currently a stub:

```rust
// crates/faf-sim/src/planner/mcts/mod.rs ~line 37 — mcts::plan (placeholder)
pub fn plan(
    _units: &Units,
    _initial_state: GraphState,
    _goal_id: &UnitKind,
    _iterations: usize,
    _value_net: ValueNetKind,
    _config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    todo!("MCTS + value-net planner is not yet implemented")
}
```

When implemented, it should:

1. Load or construct a `ValueNet`.
2. Build an `MctsSearch` with the requested iteration count.
3. Run `search` from `initial_state` using `units` for all unit knowledge.
4. Convert the best `SearchAction` into a `PlanResult`.

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
    pub first_action: Option<crate::planner::search::SearchAction>,
}
```

A reactive actor reads `first_action`, executes it, and ignores the projected `events` and `completion_time` because the real plan will be recomputed from the updated state.

## CLI usage

The strategy can be parsed from a string:

```rust
// crates/faf-sim/src/planner/core.rs ~line 117 — Strategy::from_str MCTS parsing
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

`PlannerConfig` is shared by all strategies. The current defaults are tuned for beam search:

```rust
// crates/faf-sim/src/planner/core.rs ~line 152 — PlannerConfig default
fn default() -> Self {
    Self {
        dt: 10.0,
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

Once this works, the next step is to train the value network so the search has something meaningful to evaluate.
