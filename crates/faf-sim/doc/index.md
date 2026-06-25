# FAF Build Orders with Rust and Machine Learning

> A living draft. The goal is to explore how machine learning can optimize
> build orders in *Forged Alliance Forever* (FAF), implemented in Rust. If the
> exploration succeeds, these notes can become a practical book:
> **"Machine Learning for Strategy Games in Rust"**.

## What this is

This folder documents the journey from a hand-written simulator and planner to
a learning system that discovers strong FAF build orders. We start with the
existing simulation model (`model.md`) and ask: *can we teach a program to play
the opening and mid-game economy better than a human-written heuristic?*

## Who this is for

- Rust programmers curious about machine learning.
- FAF players interested in build-order optimization.
- Machine-learning beginners who want a concrete, motivating project.

No deep math background is assumed. When we introduce an acronym or a fancy
term, we explain it in plain language and connect it back to the FAF problem.

## Reading order

| # | File | What it covers |
|---|------|----------------|
| - | [`model.md`](./model.md) | The existing graph-growth simulator model. Read this first if you have not. |
| 0 | [`00-introduction.md`](./00-introduction.md) | Why this project is interesting and what we hope to build. |
| 1 | [`01-optimization-problems-and-search.md`](./01-optimization-problems-and-search.md) | Optimization as search: why FAF build orders are hard. |
| 2 | [`02-what-is-machine-learning.md`](./02-what-is-machine-learning.md) | A gentle primer on machine learning for Rustaceans. |
| 3 | [`03-reinforcement-learning-for-beginners.md`](./03-reinforcement-learning-for-beginners.md) | Reinforcement learning concepts and algorithms explained simply. |
| 4 | [`04-faf-as-rl-environment.md`](./04-faf-as-rl-environment.md) | Mapping FAF build orders onto the RL framework. |
| 5 | [`05-approach-landscape.md`](./05-approach-landscape.md) | Survey of possible approaches, from pure RL to hybrid search. |
| 6 | [`06-mcts-value-net-plan.md`](./06-mcts-value-net-plan.md) | Concrete plan for MCTS + learned value network. |

## Status

This is a foundation, not a finished book. The numbered chapters establish
concepts and vocabulary. Later chapters will add:

- State representation with graph neural networks.
- Action masking and valid move generation.
- Reward shaping and curriculum learning.
- Training loops in Rust with libraries such as `candle` or `burn`.
- Experiments against the existing beam-search baseline.

## Current direction

The chosen path is **MCTS with a learned value network** (see chapter 6).
We will keep the existing simulator and action definitions, add state
featurization and a small neural network, then replace beam search with
closed-loop tree search.

## Conventions

- `// docref: example` marks teaching code that is not project source.
- Long acronyms are expanded on first use in each file.
- Rust types from `faf-sim` are referenced by module, e.g., `sim::GraphState`.
