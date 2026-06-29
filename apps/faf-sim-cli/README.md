## Commands

```sh
# Generate an SVG plan graph for a target unit.
cargo run --bin faf-sim -- plan uef novaxcenter
cargo run --release --bin faf-sim -- plan uef novaxcenter
# Train the macro-direction policy for a target unit.
# -e  : number of training episodes
# -m  : maximum simulator steps per episode
# -r  : resume from an existing model file (optional)
cargo run --bin faf-sim -- train -e 5000 -m 10000 uef novaxcenter
cargo run --release --bin faf-sim -- train -e 5000 -m 10000 uef novaxcenter

# Simulate a trained policy. The default strategy is greedy argmax over macro directions.
cargo run --bin faf-sim -- simulate uef novaxcenter
cargo run --release --bin faf-sim -- simulate uef novaxcenter
# Use an explicit strategy. `:greedy` (or `:deterministic`) makes the simulation reproducible.
cargo run --bin faf-sim -- simulate -s mcts:100:mlp:greedy uef novaxcenter
```

### Important: model format changed

The policy network now predicts **macro directions** (`BuildPower`, `MoreMass`, `MorePower`, `TechUp`) from state features only. Models saved before this change have a different input dimension and will fail to load with a dimension-mismatch error. Delete old checkpoints or retrain:

```sh
rm data/models/mlp-*.mpk
```

No `.trajectory.json` files are produced or used anymore.
