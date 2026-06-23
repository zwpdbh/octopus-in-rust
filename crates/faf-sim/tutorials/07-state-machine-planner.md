# 07 — The State-Machine Planner

> **Note:** This tutorial describes the old `StateMachinePolicy`, which has been
> removed. The current architecture unifies all strategies on the graph-growth
> model (`GraphState` / `BuildGraph`) via `GreedyPlanner` and `BeamPlanner` in
> `graph_planner.rs`.

The old greedy policy has been replaced by a small state machine. This document
explains how `StateMachinePolicy` decides what to build next.

---

## 1. Policy interface

A policy observes the current state and returns a list of new project requests:

```rust
// crates/faf-sim/src/heuristic.rs ~line 68 — BuildPolicy
pub trait BuildPolicy {
    fn choose_projects<'a>(
        &self,
        graph: &'a TechGraph<'a>,
        state: &EconomyState,
        owned: &[&'a Unit],
        active: &[ActiveProject],
        goal: &'a Unit,
    ) -> Vec<ProjectRequest>;
}
```

The simulator then creates `ActiveProject`s from these requests and runs one tick.

## 2. Production focus states

The state machine has three mutually exclusive focuses:

```rust
// crates/faf-sim/src/heuristic.rs ~line 82 — ProductionFocus
pub enum ProductionFocus {
    Energy,
    Mass,
    BuildPower,
}
```

| Focus | Meaning | Builds |
|---|---|---|
| `Energy` | Current build power cannot be sustained energetically. | Power generators |
| `Mass` | Energy is fine and mass income is below spendable capacity. | Mass extractors |
| `BuildPower` | Mass is piling up faster than current BP can spend it. | Engineers / factories |

## 3. Transition logic

Each tick the policy decides the focus by checking conditions in order:

```rust
// crates/faf-sim/src/heuristic.rs ~line 176 — StateMachinePolicy::focus
fn focus<'a>(
    &self,
    graph: &'a TechGraph<'a>,
    state: &EconomyState,
    owned: &[&'a Unit],
    active: &[ActiveProject],
    goal: &'a Unit,
) -> ProductionFocus {
    let bp = total_build_power(owned).0;
    let Some(stats) = self.goal_drain_per_bp(goal) else {
        return ProductionFocus::BuildPower;
    };
    let Some(drain) = compute_drain(&stats, RequestedBuildPower(1.0)) else {
        return ProductionFocus::BuildPower;
    };

    // 1. Energy sustainability check.
    let energy_drain_at_full_bp = bp * drain.energy_per_second;
    if state.net_energy_income < energy_drain_at_full_bp * self.energy_safety_margin {
        return ProductionFocus::Energy;
    }

    // 2. Mass income check: are we producing more mass than we can spend?
    let mass_drain_at_full_bp = bp * drain.mass_per_second;
    let mass_income_high = state.net_mass_income
        > mass_drain_at_full_bp * self.mass_income_headroom;
    let mass_storage_high = state.mass_storage_cap > 0.0
        && state.mass_storage > state.mass_storage_cap * self.mass_storage_high;

    if mass_income_high || mass_storage_high {
        return ProductionFocus::BuildPower;
    }

    // 3. Default: expand mass income, unless we are already at the mex cap.
    if self.current_mex_count(graph, owned, active) < self.max_mex_count {
        return ProductionFocus::Mass;
    }

    ProductionFocus::BuildPower
}
```

## 4. Policy parameters

```rust
// crates/faf-sim/src/heuristic.rs ~line 113 — StateMachinePolicy defaults
impl Default for StateMachinePolicy {
    fn default() -> Self {
        Self {
            max_mex_count: 8,
            energy_safety_margin: 1.1,
            mass_income_headroom: 1.0,
            mass_storage_high: 0.8,
            secondary_bp: RequestedBuildPower(5.0),
            goal_bp: RequestedBuildPower(1_000.0),
        }
    }
}
```

- `max_mex_count` — hard cap on mass extractors to prevent energy bankruptcy.
- `energy_safety_margin` — require net energy income to exceed full-BP drain by
  this factor before leaving the `Energy` focus.
- `mass_income_headroom` — switch to `BuildPower` when mass income exceeds this
  multiple of full-BP mass drain.
- `mass_storage_high` — also switch to `BuildPower` when storage is this full.

## 5. Picking the cheapest unit per focus

Each focus has a dedicated picker that searches the unit index:

```rust
// crates/faf-sim/src/heuristic.rs ~line 219 — pick_cheapest_energy
fn pick_cheapest_energy<'a>(...) -> Option<&'a Unit> { ... }

// crates/faf-sim/src/heuristic.rs ~line 256 — pick_cheapest_mex
fn pick_cheapest_mex<'a>(...) -> Option<&'a Unit> { ... }

// crates/faf-sim/src/heuristic.rs ~line 292 — pick_cheapest_builder
fn pick_cheapest_builder<'a>(...) -> Option<&'a Unit> { ... }
```

The `BuildPower` picker prefers engineers over factories.

## 6. Study questions

1. Why does the policy check energy sustainability before checking mass income?
2. What happens if `max_mex_count` is set to 0?
3. Why might the policy still build a factory instead of an engineer in some
   factions?

## 7. Experiment

Run the heuristic test suite:

```bash
cargo test -p faf-sim heuristic
```

Then try changing `max_mex_count` or `energy_safety_margin` and observe how the
completion time changes.

Next: [08-build-order-optimization.md](./08-build-order-optimization.md)
