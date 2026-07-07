# Reinforcement Learning with Burn: A Build-Order Optimization Tutorial

This tutorial teaches you how to train a reinforcement-learning (RL) policy in Rust using the [Burn](https://github.com/tracel-ai/burn) deep-learning framework. The concrete problem we solve is **build-order optimization** in Supreme Commander: Forged Alliance (FAF): you start with a single commander unit and must choose what to build, when, and with which engineers so that a high-value goal unit (e.g. a Monkeylord or a Novax Center) finishes as early as possible.

We do not assume you already know Burn. Each chapter introduces the Burn concepts we need as we need them: `Tensor`, `Module`, `Autodiff`, optimizers, loss functions, model recording, and inference. By the end you will have seen a complete RL training pipeline: environment simulation → featurization → policy network → REINFORCE → reactive simulation.

## What this tutorial covers

- **Burn for RL:** typed tensors, generic backends, `Module` derive, `Autodiff`, optimizers, and model persistence.
- **Environment modeling:** how we represent a deterministic RTS economy as a graph that a neural network can consume.
- **Action-space design:** reducing a combinatorial build graph to six high-level directions, then resolving each direction with a deterministic heuristic.
- **Policy network:** a small MLP that outputs direction logits over those six directions.
- **Training:** REINFORCE with return normalization, masked softmax, and timeout penalties.
- **Simulation:** running the trained policy once per tick in a closed-loop reactive simulator.
- **Integration:** wiring the planner into a CLI and an actor loop.

## Who this is for

You should be comfortable with Rust and with the basics of neural networks and policy-gradient methods. You do **not** need prior experience with Burn; chapter 1 introduces the framework.

## Tutorial chapters

1. **[Why RL for build orders?](00-why-rl.md)** — Why build-order optimization is hard, and how a learned direction policy beats hand-written heuristics.
2. **[Burn basics for RL](01-burn-basics.md)** — `Backend`, `Tensor`, `TensorData`, `Module`, `Autodiff`, optimizers, devices, and recordings. The Burn patterns used throughout training.
3. **[Modeling the environment](02-the-state-graph.md)** — `SimulationState`, the economy, the 11-D feature vector, and why we featurize instead of using a GNN.
4. **[Actions and the plan graph](03-actions-and-successors.md)** — The universal plan graph, the six `EdgeCategory` directions, and the heuristic layer that turns a direction into a concrete `SimAction`.
5. **[Building the policy network in Burn](04-value-network.md)** — A direction-only `Module` with shared backbone, `LinearConfig`, forward methods, and the `evaluate_direction` helper.
6. **[Reward shaping](05-reward-shaping.md)** — Per-step rewards for mass/energy income, storage pressure, and stalls, plus one-time tech milestones and a terminal bonus.
7. **[Training with REINFORCE](06-training-pipeline.md)** — Episode rollouts, return normalization, masked log-probabilities, and timeout penalties.
8. **[Integration and CLI](08-integration.md)** — Wiring the planner into the actor loop and running `train` / `simulate` from the command line.
9. **[Benchmarking and tuning](09-benchmarking-and-tuning.md)** — Metrics, diagnosing failure modes, and tuning reward coefficients.
10. **[The heuristic layer](10-heuristic-layer.md)** — The deterministic rules that turn each of the six network directions into a build/upgrade action.
11. **[Glossary](glossary.md)** — Terms used throughout the tutorial.
12. **[Formal model](model.md)** — Exact definitions, constraints, and assumptions (optional reference).

## Quick reference

- **Default strategy:** `policy:mlp:greedy`
- **Model path pattern:** `data/models/mlp-<faction>-<unit>` (e.g. `data/models/mlp-uef-novax-center`).
- **Run tests:** `cargo test -p faf-sim`
- **Check workspace:** `cargo check --workspace`
- **Train a policy:**
  ```text
  cargo run --release -p faf-sim-cli -- train -e 5000 -m 10000 uef novaxcenter
  ```
  Add `--quiet` to suppress output.
- **Visualise the plan graph:**
  ```text
  cargo run --release -p faf-sim-cli -- plan
  ```
- **Simulate with a trained policy:**
  ```text
  cargo run --release -p faf-sim-cli -- simulate --strategy policy:mlp:greedy uef novaxcenter
  ```

## Important notes

- Old `.mpk` model files saved before the direction-only refactor are incompatible and must be deleted and retrained. The network now consumes 11 input features and outputs 6 direction logits.
- Only `ValueNetKind::Mlp` is implemented. `Gnn` returns an error if selected.
