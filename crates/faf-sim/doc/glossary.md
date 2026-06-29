# Glossary

Short names and acronyms used across this documentation track.

| Short name | Full name | Where it appears |
|---|---|---|
| ACU | Armored Command Unit | The starting unit in every FAF build order. |
| CLI | Command Line Interface | `faf-sim-cli`, the command-line tool that runs planners. |
| CUDA | Compute Unified Device Architecture | Optional `burn` backend for GPU inference. |
| FAF | Forged Alliance Forever | The game whose build orders this project optimizes. |
| MCTS | Monte Carlo Tree Search | The search algorithm at the core of this track. |
| MLP | Multi-Layer Perceptron | The learned macro-direction policy network. |
| Macro direction | High-level build priority | One of `BuildPower`, `MoreMass`, `MorePower`, or `TechUp`. |
| Micro resolver | Rule-based action selector | Turns a macro direction into a concrete `SelectionOption`. |
| PlanGraph | Goal-specific dependency graph | Subgraph of the tech graph containing only nodes and edges relevant to the goal. |
| Policy network | Network that outputs action preferences | Maps state features to a distribution over macro directions. |
| ReLU | Rectified Linear Unit | Activation function used in the policy network. |
| REINFORCE | Policy-gradient algorithm | Used to train the MLP from its own rollouts. |
| TechGraph | Capability-level dependency graph | `Units` layer that answers "who can build whom?" |
| UCB1 | Upper Confidence Bound 1 | The selection formula used inside UCT. |
| UCT | Upper Confidence Bound applied to Trees | The tree-selection strategy used by MCTS. |
| Units | Unified unit knowledge repository | `faf-sim` abstraction over `faf-units`. |
| UpgradeTable | Hand-curated upgrade cost table | `Units` layer that answers "what upgrades into what, and for what cost?" |
| WGPU | WebGPU | Optional `burn` backend for cross-platform GPU inference. |
