# 05 — Build-Power Investment

Adding engineers increases total build power, but engineers cost time and
resources to produce. This document analyzes when the investment pays off.

---

## 1. Payback time

A T1 engineer provides `BuildRate = 5`. It costs `BuildTime = 260`, `Mass = 52`,
`Energy = 260`. If an existing builder with rate `B` builds it, the production
time is:

```text
production_time = 260 / B
```

Once finished, the engineer adds `5` build power. If that extra power is applied
to a project that would otherwise take `T` seconds, the time saved is:

```text
time_saved = T - (T * B / (B + 5))
```

The investment is profitable when the cumulative time saved exceeds the
production time.

## 2. Diminishing returns

Each additional engineer adds the same absolute BP, but the relative gain
shrinks:

| Engineers | Total BP | Relative gain |
|-----------|----------|---------------|
| 1 | 5 | — |
| 2 | 10 | 2× |
| 3 | 15 | 1.5× |
| 4 | 20 | 1.33× |
| 10 | 50 | 1.11× |

Meanwhile the drain grows linearly, so the economy must scale to keep up.

## 3. The opportunity-cost view

Every second spent building an engineer is a second not spent building the goal.
The optimal number of engineers depends on:

- How long the goal takes relative to engineer production time.
- Whether the economy can feed the extra BP.
- Whether the engineer has other uses after the goal completes.

In `faf-sim`'s simplified model we ignore travel time and lifetime reuse, so the
trade-off is purely about finishing the current goal sooner.

## 4. A rule of thumb

If a project takes much longer than the time needed to build an engineer, and the
economy is not the bottleneck, building at least one engineer is usually
profitable. The `GreedyNoStallPolicy` uses a dynamic version of this rule:

```rust
// crates/faf-sim/src/heuristic.rs ~line 123 — sustainable_bp
fn sustainable_bp(&self, state: &EconomyState, reference_unit: &Unit) -> RequestedBuildPower {
    let Some(drain) = compute_drain(reference_unit, RequestedBuildPower(1.0)) else {
        return RequestedBuildPower(f64::INFINITY);
    };

    let mass_limit = if drain.mass_per_second > 0.0 {
        state.net_mass_income / drain.mass_per_second
    } else {
        f64::INFINITY
    };
    let energy_limit = if drain.energy_per_second > 0.0 {
        state.net_energy_income / drain.energy_per_second
    } else {
        f64::INFINITY
    };

    RequestedBuildPower(mass_limit.min(energy_limit))
}
```

This estimates how much build power the current income can sustain without
stalling.

## 5. Study questions

1. Under what conditions is building a T1 engineer *not* worth it?
2. Why does the relative gain of each additional engineer decrease?
3. If the economy is the bottleneck, should you keep building engineers?

## 6. Experiment

The default `simulate` command does not add engineers. Later tutorials will show
how to compare assisted vs. unassisted timelines. For now, think about the
following: how many T1 engineers would an ACU need to assist before energy stalls
on a Monkeylord?

Next: [06-concurrent-building.md](./06-concurrent-building.md)
