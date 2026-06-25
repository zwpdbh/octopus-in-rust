# 7. Benchmarking and Tuning

A planner is only better if you can measure it. This chapter defines the benchmark suite, the metrics to track, and how to tune the search.

## Benchmark suite

Use a fixed set of goals that spans easy to hard:

| Goal | Why it matters |
|------|----------------|
| T1 pgen | Sanity check: any planner should finish quickly. |
| T1 factory + engineer | Tests basic expansion sequencing. |
| T2 factory | Tests tech progression. |
| T3 engineer | Tests deeper tech and economy scaling. |
| Monkeylord | The long-horizon stress test. |

Run each goal with each strategy multiple times and report the mean and standard deviation. Because the simulator is deterministic, variance comes from planner decisions and parameter choices, not from randomness in the environment.

## Primary metrics

1. **Completion time.** The in-game seconds when the last goal unit finishes. Lower is better.
2. **Wall-clock planning time per decision.** How long the planner takes to choose an action. MCTS should not be orders of magnitude slower than beam search.
3. **Number of simulator ticks per decision.** How much of the simulator budget the search consumes. This correlates with wall-clock time but is independent of hardware.

## Secondary metrics

- **Mass income per mass invested** at completion time. Captures economy efficiency.
- **Idle builder time.** Measures whether the planner keeps builders working.
- **Energy stall frequency.** A proxy for build-order robustness.
- **Tree size / nodes expanded.** Useful for debugging the search budget.

## Tuning `c_puct`

The UCT exploration constant is the most important hyperparameter.

- Start at `sqrt(2) ≈ 1.414`.
- If MCTS keeps exploring obviously bad branches, lower `c_puct`.
- If MCTS misses good but non-obvious branches, raise `c_puct`.
- Tune on the medium goals first; the hard goals are too slow for rapid iteration.

## Tuning iteration budget

More iterations almost always improve quality, but with diminishing returns:

- Plot completion time vs. iterations for each goal.
- Stop when doubling iterations no longer measurably improves completion time.
- For real-time use, cap wall-clock time instead.

## Diagnosing failure modes

| Symptom | Likely cause | Fix |
|---------|--------------|-----|
| MCTS is much slower than beam | Too many expansions or expensive value-net inference | Reduce iterations, batch inference, or use a smaller network. |
| MCTS finds worse plans | Value net is inaccurate or undertrained | Add more training data, especially from MCTS states. |
| MCTS explores silly actions | `c_puct` too high or value net overconfident | Lower `c_puct`, add action pruning, or train a policy prior. |
| MCTS gets stuck repeating actions | Action pruning too aggressive or successor bug | Verify `Wait` is always legal and successors cover the goal path. |
| Value net returns extreme values | Input normalization wrong or loss diverged | Check feature scaling and validation loss. |

## Robustness checks

A good planner should not be brittle. Test:

- **Small `dt` changes.** Does the plan still finish near the same time?
- **Different starting economy.** Does it adapt to slight resource variations?
- **Slightly different goal.** If the goal is a similar unit, does it reuse structure?

If the planner fails these, the value net may be overfitting to the exact training distribution.

## Reporting results

Keep a results table like this:

```text
Strategy            T1 pgen  T1 fac+eng  T2 fac  T3 eng  Monkeylord  avg ms/decision
----------------------------------------------------------------------------------
beam:50             12.3     45.2        180.1   520.4   3250.0      1.2
mcts:100 (warm)     12.5     44.8        178.5   515.2   3180.0      3.5
mcts:500 (warm)     12.1     43.9        175.3   508.7   3105.0      16.2
mcts:500 (self-play) 12.0    43.5        173.1   501.4   2980.0      16.5
```

A result is accepted only if it is faster or equal on average and not orders of magnitude slower. The final column protects against planners that trade huge compute for small gains.

## Long-term maintenance

As the simulator gains features — reclaim, multiple goals, opponent modeling — revisit the benchmarks. The value network will need retraining, and the search budget may need adjustment. The benchmark suite is the contract that keeps the planner honest.
