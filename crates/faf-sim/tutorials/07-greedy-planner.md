# 07 — The Greedy No-Stall Planner

A simulator needs a policy to decide which projects to start. This document walks
through `GreedyNoStallPolicy`, the default heuristic in `faf-sim`.

---

## 1. Policy interface

A policy observes the current state and returns a list of new project requests:

```rust
// crates/faf-sim/src/heuristic.rs ~line 68 — BuildPolicy
pub trait BuildPolicy {
    fn choose_projects<'a>(
        &self,
        graph: &'a BuildGraph<'a>,
        state: &EconomyState,
        owned: &[&'a Unit],
        active: &[ActiveProject],
        goal: &'a Unit,
    ) -> Vec<ProjectRequest>;
}
```

The simulator then creates `ActiveProject`s from these requests and runs one tick.

## 2. Policy parameters

```rust
// crates/faf-sim/src/heuristic.rs ~line 86 — GreedyNoStallPolicy
pub struct GreedyNoStallPolicy {
    pub bp_utilization_target: f64,
    pub storage_safety_fraction: f64,
    pub max_concurrent_builders: usize,
    pub max_concurrent_economy: usize,
    pub secondary_bp: RequestedBuildPower,
    pub goal_bp: RequestedBuildPower,
}
```

Default values:

```rust
// crates/faf-sim/src/heuristic.rs ~line 105 — Default impl
impl Default for GreedyNoStallPolicy {
    fn default() -> Self {
        Self {
            bp_utilization_target: 0.95,
            storage_safety_fraction: 0.15,
            max_concurrent_builders: 2,
            max_concurrent_economy: 1,
            secondary_bp: RequestedBuildPower(5.0),
            goal_bp: RequestedBuildPower(1_000.0),
        }
    }
}
```

## 3. Decision logic

Each tick the policy does three things:

1. Start the goal project if it is not active and can be built.
2. If storage is low, prioritize economy buildings.
3. If there is BP headroom, add cheap builders.

```rust
// crates/faf-sim/src/heuristic.rs ~line 244 — choose_projects
impl BuildPolicy for GreedyNoStallPolicy {
    fn choose_projects<'a>(
        &self,
        graph: &'a BuildGraph<'a>,
        state: &EconomyState,
        owned: &[&'a Unit],
        active: &[ActiveProject],
        goal: &'a Unit,
    ) -> Vec<ProjectRequest> {
        let mut requests = Vec::new();

        let goal_active = active.iter().any(|p| p.priority == ProjectPriority::Goal);
        if !goal_active && self.can_build_now(graph, owned, goal) {
            requests.push(ProjectRequest {
                target_id: goal.id.clone(),
                requested_bp: self.goal_bp,
                priority: ProjectPriority::Goal,
            });
        }

        let mass_low = state.mass_storage_cap > 0.0
            && state.mass_storage < state.mass_storage_cap * self.storage_safety_fraction;
        let energy_low = state.energy_storage_cap > 0.0
            && state.energy_storage < state.energy_storage_cap * self.storage_safety_fraction;

        let active_economy = active
            .iter()
            .filter(|p| p.priority == ProjectPriority::Economy)
            .count();

        if (mass_low || energy_low) && active_economy < self.max_concurrent_economy {
            if let Some(econ) = self.pick_cheapest_economy(graph, owned, goal) {
                requests.push(ProjectRequest {
                    target_id: econ.id.clone(),
                    requested_bp: self.secondary_bp,
                    priority: ProjectPriority::Economy,
                });
            }
        }

        let active_builders = active
            .iter()
            .filter(|p| p.priority == ProjectPriority::Builder)
            .count();

        let reference = if compute_drain(goal, RequestedBuildPower(1.0)).is_some() {
            goal
        } else {
            // Fall back to the first T1 engineer in the index.
            graph
                .index()
                .units
                .iter()
                .find(|u| {
                    u.has_category("ENGINEER")
                        && u.has_category("TECH1")
                        && goal.faction().map_or(true, |f: &str| {
                            u.faction()
                                .map_or(true, |uf: &str| uf.eq_ignore_ascii_case(f))
                        })
                })
                .unwrap_or(goal)
        };

        let owned_bp = self.owned_bp(owned);
        let sustainable = self.sustainable_bp(state, reference);
        let target_bp = sustainable.0 * self.bp_utilization_target;

        if owned_bp.0 < target_bp && active_builders < self.max_concurrent_builders {
            if let Some(builder) = self.pick_cheapest_builder(graph, owned, goal) {
                requests.push(ProjectRequest {
                    target_id: builder.id.clone(),
                    requested_bp: self.secondary_bp,
                    priority: ProjectPriority::Builder,
                });
            }
        }

        requests
    }
}
```

## 4. Picking cheap builders and economy

The policy picks the builder with the lowest build-time per build-power gained:

```rust
// crates/faf-sim/src/heuristic.rs ~line 169 — pick_cheapest_builder
fn pick_cheapest_builder<'a>(
    &self,
    graph: &'a BuildGraph<'a>,
    owned: &[&'a Unit],
    goal: &'a Unit,
) -> Option<&'a Unit> {
    let goal_faction = goal.faction();
    graph
        .index()
        .units
        .iter()
        .filter(|u| {
            u.economy.as_ref().and_then(|e| e.build_rate).unwrap_or(0.0) > 0.0
                && self.can_build_now(graph, owned, u)
                && match goal_faction {
                    Some(f) => u.faction().map_or(true, |uf: &str| uf.eq_ignore_ascii_case(f)),
                    None => true,
                }
        })
        .min_by(|a, b| {
            let a_econ = a.economy.as_ref().unwrap();
            let b_econ = b.economy.as_ref().unwrap();
            let a_time_per_bp = a_econ.build_time.unwrap() / a_econ.build_rate.unwrap();
            let b_time_per_bp = b_econ.build_time.unwrap() / b_econ.build_rate.unwrap();
            a_time_per_bp
                .partial_cmp(&b_time_per_bp)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}
```

## 5. Tuning the policy

- Lower `bp_utilization_target` → more conservative, fewer builders, less stall risk.
- Higher `storage_safety_fraction` → build economy sooner.
- Higher `max_concurrent_builders` → more aggressive BP growth.

## 6. Study questions

1. Why does the goal project request `1000` BP instead of exactly the available amount?
2. What happens if `bp_utilization_target` is set to `1.0`?
3. Why might the cheapest builder not always be a T1 engineer?

## 7. Experiment

Run the heuristic test suite:

```bash
cargo test -p faf-sim heuristic
```

Then try tweaking the default parameters in a small test and observe how the
completion time changes.

Next: [08-build-order-optimization.md](./08-build-order-optimization.md)
