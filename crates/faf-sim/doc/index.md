# Optimizing FAF Build Orders with MCTS

This track teaches how to use **Monte Carlo Tree Search (MCTS) with a learned macro-direction policy** to optimize *Forged Alliance Forever* build orders. We assume you already know Rust and the basics of FAF; we do not re-explain what a neural network is or what reinforcement learning means in the abstract.

The focus is practical: how to frame the simulator as an MCTS search problem, how to train a policy that selects high-level build priorities from economy/state features, how to resolve those priorities into concrete build commands, how to implement UCT search in Rust, and how to wire everything into the existing `faf-sim` planner.

## What you will build

1. A state representation that the MCTS can reason about.
2. A compact, legal option generator derived from the `PlanGraph`.
3. A small neural network that maps state features to one of four macro directions: `BuildPower`, `MoreMass`, `MorePower`, or `TechUp`.
4. A deterministic resolver that turns the chosen macro direction into a concrete `SelectionOption`.
5. A policy-gradient training loop that turns simulator rollouts into network weights.
6. A UCT search that uses the policy network to choose the next command.
7. A benchmark suite that compares MCTS against the existing baseline.

## Reading order

| # | File | What it covers |
|---|------|----------------|
| 0 | [`00-why-mcts.md`](./00-why-mcts.md) | Why MCTS + learned policy fits FAF build-order optimization. |
| 1 | [`01-the-state-graph.md`](./01-the-state-graph.md) | `GraphState` as the MCTS state: units, builders, economy, stall. |
| 2 | [`02-actions-and-successors.md`](./02-actions-and-successors.md) | `SelectionOption`, `SelectionPools`, successor generation, action masking. |
| 3 | [`03-value-network.md`](./03-value-network.md) | State featurization, macro-direction network architecture, resolver, REINFORCE training with `burn`. |
| 4 | [`04-mcts-search.md`](./04-mcts-search.md) | Current one-step macro policy, planned UCT selection, expansion, leaf evaluation, backup, tree reuse. |
| 5 | [`05-integration.md`](./05-integration.md) | Wiring `Strategy::Mcts` into `Planner` and the CLI. |
| 6 | [`06-training-pipeline.md`](./06-training-pipeline.md) | REINFORCE rollouts, reward shaping, curriculum, future self-play. |
| 7 | [`07-benchmarking-and-tuning.md`](./07-benchmarking-and-tuning.md) | Metrics, benchmark suite, diagnosing and tuning the search. |

## Reference

- [`model.md`](./model.md) — The full formal model: nodes, edges, builder constraints, economy, stall, objectives, and assumptions. Read this if you need the exact rules behind the simulator.
- [`glossary.md`](./glossary.md) — Acronyms and short names used throughout this track.

## Conventions

- Code snippets taken from project source begin with a source-location comment:
  ```rust
  // crates/faf-sim/src/planner/core.rs ~line 105 — Strategy enum
  ```
- Conceptual or teaching-only snippets are marked `// docref: example`.
- Acronyms are expanded on first use in each file.
