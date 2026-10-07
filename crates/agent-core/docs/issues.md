
## The two loops are parallel implementations of the same core

Both `run_turn_loop` (brain.rs:172) and `run_step_loop` (brain.rs:308) contain:

- **identical toolset construction** (~15 lines, byte-for-byte the same: `ApprovalToolset` + `HookAwareToolset` wrapping)
- **identical checkpoint logic** (the `StepContext` + `checkpoint_policy` block)
- the same step-loop skeleton

They presumably share `execute_step`/`run_single_step_with_retry` deeper down, but the loop *bookkeeping* is written twice.

## The divergence bugs this has already caused

This is why duplication matters — the two copies have already drifted apart:

1. **`run_step_loop` has no step cap.** The turn loop is bounded: `for step_no in 0..config.max_steps_per_turn` (brain.rs:222). The step loop is a bare `loop {}` — if the model keeps emitting tool calls forever, it runs forever. Your turn API is protected; your step API isn't.
2. **`step_no` never increments in `run_step_loop`** (brain.rs:327: `let step_no = 0;` — immutable). Every step reports `StepBegin { n: 0 }` and every checkpoint gets `step_no: 0`. Consumers counting steps get garbage.
3. Any fix you make to the turn loop (bounded retry, cancellation) you must remember to make again in the step loop — the classic two-copies drift hazard.

## The fix direction (when you get there)

Unify: extract the shared step-loop into one function that both entry points call, parameterized by "turn context":

```
run_turn_loop = TurnBegin → prompt hook → push user msg → shared_step_loop(ctx) → TurnEnd
run_step_loop =                                        shared_step_loop(ctx)
```

One owner for: step counting, `max_steps_per_turn`, checkpoints, retry budget, and (soon) the cancellation token. This mirrors kimi-code's structure — there is exactly **one** turn machine; different entry points feed it, but nobody maintains a second loop.

**My advice: do this unification as part of fix 1.1**, before touching retry semantics. Otherwise you'll implement bounded retry in `run_single_step_with_retry`, then discover the step-loop bookkeeping around it also needs the budget — in two places. Fix the structure first, then the semantics lands once.

Add to your bug list: *step loop unbounded (no `max_steps_per_turn`)* and *step counter stuck at 0 in `run_step_loop`* — both found by your own inspection, which is exactly the kind of thing this exercise is meant to train.
