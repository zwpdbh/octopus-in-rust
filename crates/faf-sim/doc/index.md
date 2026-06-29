# faf-sim documentation

`faf-sim` is a discrete-time build-order planner and simulator for Supreme Commander: Forged Alliance. It models economy, unit construction, and adjacency bonuses, and learns a hierarchical policy to guide MCTS search.

## Reading guide

Start with the chapters in order if you are new to the crate:

1. **[Why MCTS?](00-why-mcts.md)** — why MCTS is a good fit for FAF build-order optimization.
2. **[The state graph](01-the-state-graph.md)** — how the simulator state is represented and what MCTS sees.
3. **[Actions and successors](02-actions-and-successors.md)** — legal actions, plan-graph edges, and multi-builder squads.
4. **[Hierarchical policy network](03-value-network.md)** — the three learned networks that guide planning.
5. **[MCTS search](04-mcts-search.md)** — the planned UCT loop and current one-step policy.
6. **[Integration](05-integration.md)** — how the planner connects to actors and the CLI.
7. **[Training pipeline](06-training-pipeline.md)** — how the policy bundle is trained, saved, and loaded.
8. **[Benchmarking and tuning](07-benchmarking-and-tuning.md)** — metrics and search-budget tuning.
9. **[Build-order optimization model](model.md)** — formal definitions, constraints, and assumptions.
10. **[Glossary](glossary.md)** — terms and abbreviations used throughout the docs.

## Quick reference

- **Default strategy:** `mcts:100:mlp:greedy`
- **Model path pattern:** `data/models/mlp-<faction>-<unit>`
- **Run tests:** `cargo test -p faf-sim`
- **Check workspace:** `cargo check --workspace`
- **Train a policy:** `cargo run --release --bin faf-sim -- train -e 5000 -m 10000 uef novaxcenter`  
  Add `--quiet` to suppress output, or `--patience 1000` to stop when no improvement is seen for 1000 episodes.
- **Simulate with a trained policy:** `cargo run --release --bin faf-sim -- simulate --strategy mcts:100:mlp:greedy uef novaxcenter`

## Important notes

- Old `.mpk` model files saved before the hierarchical-policy redesign are incompatible and must be deleted and retrained.
- The MCTS search loop is not yet implemented; `Strategy::Mcts` currently runs a one-step hierarchical policy.
- Only `ValueNetKind::Mlp` is implemented. `Gnn` returns an error if selected.
