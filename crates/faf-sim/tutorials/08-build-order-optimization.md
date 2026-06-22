# 08 — Build-Order Optimization as Search

So far we have a simulator and a state-machine heuristic. This document frames
the full optimization problem and sketches algorithms that could solve it.

---

## 1. Problem statement

Given:

- A starting set of units (usually one ACU).
- A goal unit `G`.
- A unit database with build times, costs, and builder categories.

Find a schedule of build actions that produces `G` in minimum wall-clock time.

A schedule is a sequence of decisions at discrete times:

1. Which units to start building.
2. How much build power to assign to each.
3. When to stop/switch projects.

## 2. Search space

Even under the simplifications of `faf-sim`, the search space is large:

- At any moment you can start any unit you have a builder for.
- Build power can be split continuously among active projects.
- Projects can be paused or cancelled.

In practice we discretize:

- Time steps of length `dt`.
- A finite set of candidate units to build (pruned by the dependency graph).
- A fixed allocation policy once projects are chosen.

## 3. Lower bounds

A useful lower bound for any planner is the **resource-free completion time**:

```text
time >= total_work / total_build_power
```

where `total_work` is the sum of `BuildTime` for all units that must be built,
and `total_build_power` is the peak BP ever available.

Another lower bound is the **resource-limited completion time**: the earliest time
at which cumulative income equals cumulative cost. The true optimum is at least
the maximum of these bounds.

## 4. Algorithm families

### 4.1 State-machine heuristics

Start with what we already have: `StateMachinePolicy`. Fast, no search, but not
optimal.

### 4.2 Beam search

Keep the best `k` partial schedules ranked by a heuristic (completion time lower
bound plus estimated remaining cost). Expand each by trying a small set of next
actions.

### 4.3 A* / branch-and-bound

Use admissible heuristics to prune schedules that cannot beat the best found so
far. The continuous-drain economy makes admissible heuristics tricky because
stalls are non-linear.

### 4.4 Mathematical programming

For very coarse discretizations, the problem can be formulated as a mixed-integer
program. The continuous-drain constraints introduce bilinear terms
(build-power × resource-drain), which are hard.

### 4.5 Reinforcement learning

The state is `(owned_units, active_projects, economy_state)`. The action space
is which project to start or how to reallocate BP. The reward is negative elapsed
time, with a large bonus for completing the goal.

## 5. Evaluation methodology

Whatever algorithm you choose, compare against:

- **ACU-alone baseline**: build the goal directly with the starting ACU.
- **Observed-economy baseline**: derive the economy from the starting units and
  compute the resource-limited completion time
  (see [03-sequential-baseline.md](./03-sequential-baseline.md)).
- **State-machine heuristic**: the default `StateMachinePolicy`.

Only improvements over all three are meaningful.

## 6. Study questions

1. Why is the continuous-drain model harder to optimize than a "pay upfront"
model?
2. Propose an admissible heuristic for A* in this problem.
3. What makes RL attractive here, and what makes it difficult?

## 7. Experiment

The `plan` subcommand is a placeholder:

```rust
// apps/faf-sim-cli/src/main.rs ~line 260 — run_plan
fn run_plan(target: ResearchTarget, strategy: &str) {
    println!(
        "Plan target: {} strategy: {}",
        target.display_name(),
        strategy
    );
    println!("(Planner not yet implemented.)");
}
```

Pick one algorithm family and sketch how you would implement it. Where would it
live in the crate tree?

Next: [09-known-limitations.md](./09-known-limitations.md)
