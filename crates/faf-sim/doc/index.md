# Reinforcement Learning for Build-Order Optimization with Burn

This tutorial walks through building a reinforcement-learning (RL) planner in Rust with the [`Burn`](https://github.com/tracel-ai/burn) deep-learning framework. The concrete domain is Supreme Commander: Forged Alliance (FAF): you start with a single commander unit and must choose what to build, when, and with which engineers so that a high-value goal (e.g. a Monkeylord or a Novax Center) finishes as early as possible.

`faf-sim` provides:

- A deterministic discrete-time simulator for economy, construction, and adjacency bonuses.
- A universal plan graph that encodes every legal prerequisite chain.
- A hierarchical policy network written as a `burn::module::Module`.
- A REINFORCE training loop that trains the network directly from episode rollouts.
- A UCT MCTS search that uses the trained network as a prior and rollout policy during simulation.

## Who this tutorial is for

You should be comfortable with Rust and with the basics of neural networks and policy-gradient methods. You do **not** need prior experience with Burn; the first few chapters introduce the framework as we use it.

## Tutorial chapters

1. **[Why RL for build orders?](00-why-mcts.md)** — What makes build-order optimization hard, and why we combine MCTS with a learned policy.
2. **[Burn basics for RL](01-burn-basics.md)** — `Backend`, `Tensor`, `Module`, `Autodiff`, optimizers, devices, and recordings. Everything you need to read the rest of the code.
3. **[Modeling the environment](02-the-state-graph.md)** — `SimulationState`, the economy, featurization, and what the network sees.
4. **[Actions and the plan graph](03-actions-and-successors.md)** — How we reduce a huge raw action space to a small set of legal plan-graph edges.
5. **[Building the policy network in Burn](04-value-network.md)** — A hierarchical `Module` with shared backbone, upgrade head, direction head, action head, build-power head, and engineer-squad head.
6. **[Reward shaping](05-reward-shaping.md)** — Per-step rewards for build-power, mass/power income, storage pressure, and stalls, plus one-time tech milestones and a terminal bonus.
7. **[Training with REINFORCE](06-training-pipeline.md)** — Episode rollouts with the policy network (no MCTS), returns, joint log-probabilities, entropy regularization, and fine-tuning on the best trajectory.
8. **[MCTS search](07-mcts-search.md)** — UCT selection, expansion, policy-prior ordering, rollout, backup, and choosing the final action.
9. **[Integration and CLI](08-integration.md)** — Wiring the planner into the actor loop and running `train` / `simulate` from the command line.
10. **[Benchmarking and tuning](09-benchmarking-and-tuning.md)** — Metrics, diagnosing failure modes, and tuning `c_puct`, iteration budgets, and reward coefficients.
11. **[Glossary](glossary.md)** — Terms used throughout the tutorial.
12. **[Formal model](model.md)** — Exact definitions, constraints, and assumptions (optional reference).

## Quick reference

- **Default strategy:** `mcts:100:mlp:greedy`
- **Model path pattern:** `data/models/mlp-<faction>-<unit>` (e.g., `data/models/mlp-uef-novax-center`). Models are saved per target for convenience, but the network shape is fixed for all goals of the same tech level.
- **Run tests:** `cargo test -p faf-sim`
- **Check workspace:** `cargo check --workspace`
- **Train a policy:** `cargo run --release -p faf-sim-cli -- train -e 5000 -m 10000 uef novaxcenter`  
  Add `--quiet` to suppress output, `--patience 1000` to stop when no improvement is seen for 1000 episodes, or `--no-epsilon-decay` to keep exploration constant.
- **Visualise the plan graph:** `cargo run --release -p faf-sim-cli -- plan`  
  The SVG includes a placeholder **Target** node representing the T3-engineer-only goal edge.
- **Simulate with a trained policy:** `cargo run --release -p faf-sim-cli -- simulate --strategy mcts:100:mlp:greedy uef novaxcenter`

## Important notes

- Old `.mpk` model files saved before the abstract-goal redesign are incompatible and must be deleted and retrained. The universal plan graph now has a fixed edge count and a synthetic `Goal` node.
- Only `ValueNetKind::Mlp` (the hierarchical network) is implemented. `Gnn` returns an error if selected.
