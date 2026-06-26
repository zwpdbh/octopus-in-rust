# 5. Integration

This chapter explains how to wire the MCTS planner into the existing `faf-sim` planner enum and run it from the CLI (Command Line Interface).

## Strategy enum

The `Planner` already dispatches to three strategies:

```rust
// crates/faf-sim/src/planner/core.rs ~line 75 — Strategy enum
pub enum Strategy {
    /// Greedy: pick the single best successor state at each step.
    Greedy,
    /// Beam search: keep the top-K most promising states each layer.
    Beam {
        /// Number of states kept after each search layer.
        beam_width: usize,
    },
    /// Monte Carlo Tree Search guided by a learned value network.
    Mcts {
        /// Number of MCTS iterations to run per decision.
        iterations: usize,
    },
}
```

The MCTS variant is already declared. The missing piece is the implementation of `mcts::plan`.

## Entry point

`Planner::plan` matches on the strategy and forwards to the corresponding module:

```rust
// crates/faf-sim/src/planner/core.rs ~line 198 — Planner::plan dispatch
pub fn plan(
    &self,
    index: &DataIndex,
    initial_state: GraphState,
    goal: &Unit,
) -> Result<PlanResult, PlannerError> {
    match self.strategy {
        Strategy::Greedy => greedy::plan(index, initial_state, goal, &self.config),
        Strategy::Beam { beam_width } => {
            beam::plan(index, initial_state, goal, beam_width, &self.config)
        }
        Strategy::Mcts { iterations } => {
            mcts::plan(index, initial_state, goal, iterations, &self.config)
        }
    }
}
```

The MCTS entry point is currently a stub:

```rust
// crates/faf-sim/src/planner/mcts/mod.rs ~line 38 — mcts::plan (placeholder)
pub fn plan(
    _index: &DataIndex,
    _initial_state: GraphState,
    _goal: &Unit,
    _iterations: usize,
    _config: &PlannerConfig,
) -> Result<PlanResult, PlannerError> {
    todo!("MCTS + value-net planner is not yet implemented")
}
```

When implemented, it should:

1. Load or construct a `ValueNet`.
2. Build an `MctsSearch` with the requested iteration count.
3. Run `search` from `initial_state`.
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
    return Ok(Strategy::Mcts { iterations: 100 });
}
if let Some(rest) = lower.strip_prefix("mcts:") {
    if let Ok(iterations) = rest.parse::<usize>() {
        return Ok(Strategy::Mcts { iterations });
    }
}
```

So the CLI can accept arguments such as:

```text
faf-sim-cli plan --strategy mcts --goal URL0402
faf-sim-cli plan --strategy mcts:500 --goal URL0402
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
let planner = Planner::new(Strategy::Mcts { iterations: 100 });
let result = planner.plan(&index, initial_state, goal)?;

if let Some(action) = result.first_action {
    println!("next action: {:?}", action);
}
```

Once this works, the next step is to train the value network so the search has something meaningful to evaluate.
