# 9. Benchmarking and Tuning

A planner is only better if you can measure it. This chapter defines the benchmark suite, the metrics to track, and how to tune the policy.

## Benchmark suite

Use a fixed set of goals that spans easy to hard:

| Goal | Why it matters |
|------|----------------|
| T1 pgen | Sanity check: any planner should finish quickly. |
| T1 factory + engineer | Tests basic expansion sequencing. |
| T2 factory | Tests tech progression. |
| T3 engineer | Tests deeper tech and economy scaling. |
| Monkeylord | The long-horizon stress test. |

Run each goal multiple times and report the mean and standard deviation. Because the simulator is deterministic, variance comes from policy decisions and parameter choices, not from randomness in the environment.

## Primary metrics

1. **Completion time.** The in-game seconds when the goal finishes. Lower is better.
2. **Goal reach rate.** The fraction of episodes that reach the goal within the step budget. Higher is better.
3. **Wall-clock planning time per decision.** How long the planner takes to choose an action. The one-step policy should be fast enough to keep up with the simulator.
4. **Mass-income growth.** The per-step reward the trainer observes. Useful for diagnosing whether the policy is learning the only active signal.

## Secondary metrics

- **Mass income per mass invested** at completion time. Captures economy efficiency.
- **Idle builder time.** Measures whether the policy keeps builders working.
- **Energy stall frequency.** A proxy for build-order robustness.

## Tuning exploration

Training currently uses greedy action selection, so the main levers for escaping local optima are:

- `TrainConfig::reward_mass_income_coef` — a larger coefficient makes mass-income changes more influential, which can pull the policy out of directions that stall the economy.
- Network capacity and training budget — a larger network or more episodes can discover directions the current policy misses.

A separate exploration mechanism will be added later (e.g. temperature-based sampling or parameter-space noise).

## Tuning the network

The direction-only network is small by default (11 inputs, 128-D hidden, 64-D latent, 6 outputs). If the policy underfits:

- Increase `backbone_hidden` and `latent_dim` in `macro_net.rs`.
- Train for more episodes with a larger `max_steps` budget.

If the policy overfits:

- Reduce network sizes.
- Train on multiple goals rather than a single goal.
- Adjust `reward_mass_income_coef` so the policy is not rewarded for trivial behaviors.

## Tuning reward coefficients

The per-step reward is currently just the mass-income delta:

- If the policy ignores mass growth entirely, raise `reward_mass_income_coef`.
- If the policy builds too many extractors and never techs, the mass-only baseline is behaving as expected: it has no goal-seeking term yet. Add a goal-completion bonus or milestone term when you are ready to extend the reward.
- If single steps produce huge losses or `NaN`, lower `reward_mass_income_coef` or enable gradient clipping.

Because each step is updated individually, the absolute scale of the coefficient maps directly into gradient scale.

## Diagnosing failure modes

| Symptom | Likely cause | Fix |
|---------|--------------|-----|
| Policy is much slower than expected | Expensive per-step inference or very small `dt` | Use a smaller network or increase `dt`. |
| Policy finds worse plans than training best | Policy undertrained; the saved model is the final parameters, not a separately tracked best model | Train longer, or add goal-seeking reward terms. |
| Policy explores silly actions | Exploration mechanism (when added) is too aggressive | Reduce the exploration strength or temperature. |
| Policy gets stuck repeating actions | Heuristic returns `Wait` for every direction, or successor bug | Verify `Wait` is always legal, the direction mask is non-empty when actions exist, and the heuristic covers the goal path. |
| Policy network returns extreme values | Input normalization wrong or loss diverged | Check feature scaling, learning rate, and validation loss. |
| Policy never reaches the goal | Reward signal is mass-only, so the policy is not directly encouraged to finish the goal | Add a goal-completion reward, or accept that the baseline is economy-only. |
| Heuristic always returns `Wait` for a direction | `is_direction_legal` or `direction_to_action` mismatch | Add a unit test for that direction from the ACU start state and step through the heuristic. |
| Simulation with random weights is very slow | No trained model was found; the policy explores a huge horizon with random directions | Train a policy first, or use a tiny `max_sim_time` for smoke tests. |

## Robustness checks

A good policy should not be brittle. Test:

- **Small `dt` changes.** Does the plan still finish near the same time?
- **Different starting economy.** Does it adapt to slight resource variations?
- **Slightly different goal.** If the goal is a similar unit, does it reuse structure?

If the policy fails these, it may be overfitting to the exact training distribution.

## Reporting results

Keep a results table like this:

```text
Strategy              T1 pgen  T1 fac+eng  T2 fac  T3 eng  Monkeylord  avg ms/decision
------------------------------------------------------------------------------------
policy:greedy         12.3     45.2        180.1   520.4   3250.0      1.2
```

A result is accepted only if it is faster or equal on average and not orders of magnitude slower. The final column protects against planners that trade huge compute for small gains.

## Long-term maintenance

As the simulator gains features — reclaim, multiple goals, opponent modeling — revisit the benchmarks. The policy network will need retraining. The benchmark suite is the contract that keeps the planner honest.
