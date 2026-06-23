# 09 — Known Limitations and Roadmap

> **Note:** This tutorial references the old `HeuristicSimulator` and
> `StateMachinePolicy`, which have been removed. The current implementation uses
> `GreedyPlanner` and `BeamPlanner` over the graph-growth model (`GraphState` /
> `BuildGraph`). The listed limitations still apply unless noted otherwise.

`faf-sim` is a research simulator, not a full game engine. This document lists
the simplifications it makes and maps a path from the current baseline to a more
complete optimizer.

---

## 1. Current simplifications

- **Travel time** — engineers and factories are assumed to be in place.
- **Reclaim** — no map reclaim is modeled.
- **Combat, scouting, and map control** — purely economic simulation.
- **Unit upgrades / enhancements** — ACU T2/T3 engineering upgrades are not
  modeled as separate prerequisites (yet).
- **Concurrent projects** — `HeuristicSimulator` ticks projects sequentially,
  which is an approximation of true simultaneous drain.
- **Engineer production for assist** — the `StateMachinePolicy` builds engineers
  or factories when mass income exceeds current spendable capacity.
- **Power/mass infrastructure beyond prerequisites** — the policy now builds
  optional mass extractors and power generators, but with a simple cap/margin
  rule rather than a full ROI model.

## 2. What is already modeled

- Continuous-drain economy with mass and energy stalls.
- Build power, build time, and remaining-work tracking.
- Builder prerequisites via faction-scoped category matching.
- A concurrent simulator with pluggable policies.
- A state-machine heuristic policy with energy / mass / build-power focus.

## 3. Near-term roadmap

1. **Better economy infrastructure policy** — replace the mex cap and energy
   margin with a proper ROI model for mass extractors and power generators.
2. **Reclaim model** — add a simple reclaim income source.
3. **ACU enhancements** — model engineering upgrades as build-power and economy
   modifiers.
4. **Travel time** — add time for engineers to reach a project and for factories
   to be placed.
5. **Search-based planner** — implement beam search or A* over build orders.
6. **RL environment** — expose the simulator as a Gym-like environment.

## 4. Validation principle

Every new feature should make the predicted completion time closer to a real
replay, or make the optimizer find a schedule that a human player would recognize
as good. If a feature complicates the model without improving either metric, it
should stay optional.

## 5. Study questions

1. Which simplification has the largest impact on predicted Monkeylord timing?
2. How would you validate a new planner against real games?
3. Should travel time be added before or after a search-based planner? Why?

## 6. Closing experiment

Run the full `faf-sim` test suite to confirm the current mechanics are stable:

```bash
cargo test -p faf-sim
```

Then open the source files referenced throughout this series and trace how a
single tick updates `EconomyState`.

---

End of the series. Start over at [01-ground-facts.md](./01-ground-facts.md).
