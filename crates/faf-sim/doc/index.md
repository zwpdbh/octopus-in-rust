# Optimizing FAF Build Orders with MCTS

This track teaches how to use **Monte Carlo Tree Search (MCTS) with a learned value network** to optimize *Forged Alliance Forever* build orders. We assume you already know Rust and the basics of FAF; we do not re-explain what a neural network is or what reinforcement learning means in the abstract.

The focus is practical: how to frame the simulator as an MCTS search problem, how to train a value network that estimates state quality, how to implement UCT search in Rust, and how to wire everything into the existing `faf-sim` planner.

## What you will build

1. A state representation that the MCTS can reason about.
2. A compact, legal action generator.
3. A small neural network that predicts how much time remains from any state.
4. A UCT search that uses that network to choose the next build/upgrade/assist/wait command.
5. A training pipeline that turns simulator rollouts into value-net weights.
6. A benchmark suite that compares MCTS against the existing beam-search baseline.

## Reading order

| # | File | What it covers |
|---|------|----------------|
| 0 | [`00-why-mcts.md`](./00-why-mcts.md) | Why MCTS + value net fits FAF build-order optimization. |
| 1 | [`01-the-state-graph.md`](./01-the-state-graph.md) | `GraphState` as the MCTS state: units, builders, economy, stall. |
| 2 | [`02-actions-and-successors.md`](./02-actions-and-successors.md) | `SearchAction`, legal moves, successor generation, action masking. |
| 3 | [`03-value-network.md`](./03-value-network.md) | Featurization, network architecture, supervised training with `burn`. |
| 4 | [`04-mcts-search.md`](./04-mcts-search.md) | UCT selection, expansion, leaf evaluation, backup, tree reuse. |
| 5 | [`05-integration.md`](./05-integration.md) | Wiring `Strategy::Mcts` into `Planner` and the CLI. |
| 6 | [`06-training-pipeline.md`](./06-training-pipeline.md) | Data generation, warm-start, self-play, policy prior. |
| 7 | [`07-benchmarking-and-tuning.md`](./07-benchmarking-and-tuning.md) | Metrics, benchmark suite, diagnosing and tuning the search. |

## Reference

- [`model.md`](./model.md) — The full formal model: nodes, edges, builder constraints, economy, stall, objectives, and assumptions. Read this if you need the exact rules behind the simulator.
- [`glossary.md`](./glossary.md) — Acronyms and short names used throughout this track.

## Conventions

- Code snippets taken from project source begin with a source-location comment:
  ```rust
  // crates/faf-sim/src/planner/core.rs ~line 75 — Strategy enum
  ```
- Conceptual or teaching-only snippets are marked `// docref: example`.
- Acronyms are expanded on first use in each file.
