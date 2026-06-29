# Glossary

| Term | Meaning |
| --- | --- |
| **Action space** | All legal plan-graph edges from the current `GraphState`, plus the wait action. |
| **Adjacency bonus** | Reduced energy/mass cost or build time when structures are placed near each other. Encoded in `Units` and applied to build projects. |
| **Backend** | The Burn compute backend. CPU training uses `NdArray`, and GPU training can use `Wgpu`. Aliased as `TrainBackend` in the training modules. |
| **Baseline** | Moving average of recent episode returns used to center REINFORCE advantages and reduce variance. |
| **Build-power network** | Learned network that decides how much total build power to allocate to a selected plan-graph edge. |
| **Build order** | A sequence of construction actions that takes the economy from the starting commander to a target unit. |
| **BuildPower** | Total construction rate contributed by idle engineers, measured in build points per second. |
| **Burn** | The Rust deep-learning framework used by `faf-sim` for training and inference. |
| **Command** | An actor message that tells the simulator to execute a build, upgrade, or assist action with a specific squad of builders. |
| **C_puct** | UCT exploration constant controlling the trade-off between exploitation and exploration. |
| **Deterministic policy** | At inference time, always picks the highest-scoring macro edge and rounds power/squad values. Used for evaluation. |
| **Episode** | One full rollout from the starting state until the goal is reached or `max_steps` is exceeded. |
| **Engineer-squad network** | Learned network that decides the `[T1, T2, T3]` counts of engineers assigned to a selected edge. |
| **Expansion** | In MCTS, generating a child node by applying an untried legal action to a parent state. |
| **Faction** | One of UEF, Cybran, Aeon, or Seraphim. Determines available units and build trees. |
| **Feature vector** | Fixed-size numerical representation of a `GraphState` plus optional shortfall feedback, fed into the learned networks. |
| **Fine-tuning** | Supervised training on the best trajectory found during REINFORCE, run after the main loop. |
| **GraphState** | The authoritative discrete simulator state: units, projects, economy, completed structures. |
| **GNN** | Graph Neural Network. A planned alternative value head that reasons directly over the plan graph. Not yet implemented. |
| **Goal unit** | The target `UnitKind` the planner is trying to build, e.g. `novaxcenter` or `monkeylord`. |
| **Macro network** | Learned network that selects a concrete plan-graph edge given state features and shortfall feedback. |
| **Macro edge** | A concrete, executable edge in the `PlanGraph`, such as "build a T1 land factory" or "upgrade a T1 mass extractor to T2". |
| **MLP** | Multi-Layer Perceptron. The currently implemented value/policy head, now a three-network bundle. |
| **MCTS** | Monte Carlo Tree Search. The planned search algorithm that will use the hierarchical policy as a prior and rollout policy. |
| **NodeId** | Opaque identifier for a node in `GraphState` (a unit instance or project). |
| **Observation** | Actor message carrying either a `BuildEvent` or the full `GraphState` from the simulator to the decision actor. |
| **PlanGraph** | Directed acyclic graph of units and prerequisites used to enumerate legal macro edges. |
| **Planner** | The public facade that owns the strategy, configuration, policy bundle, and shortfall state. |
| **Policy bundle** | The three learned networks saved and loaded together: macro net, build-power net, and engineer-squad net. |
| **PolicyBundle** | Burn module that groups `MacroNet`, `BuildPowerNet`, and `EngineerSquadNet`. |
| **REINFORCE** | Policy-gradient algorithm used to train the three networks from Monte Carlo returns. |
| **Patience** | Number of episodes without a new best completion time after which training stops early. |
| **Return** | Discounted sum of rewards over an episode, used as the target for REINFORCE. |
| **Selection** | In MCTS, traversing the tree from the root to a leaf using the UCB1 formula. |
| **Shortfall** | Unmet engineer demand from the previous tick, fed back into the macro network as a three-dimensional feature. |
| **SimAction** | Internal representation of a simulator action: `Build`, `Upgrade`, `Assist`, or `Wait`. |
| **Simulator tick** | One fixed-duration step of the discrete-time economy and build-progress simulation. |
| **Strategy** | The planner enum. Currently only `Strategy::Mcts` exists, with `ValueNetKind::Mlp` or `Gnn`. |
| **UCT** | Upper Confidence Bound applied to Trees. The selection formula used by MCTS. |
| **UnitKind** | Type-level identifier for a unit class, e.g. `UnitKind::Unique(UnitId("UEL0301".to_string()))`. |
| **Units** | The static unit database: stats, build lists, prerequisites, adjacency bonuses. |
| **Upgrade** | Replacing an existing structure with a higher-tech version, such as T1 mex → T2 mex. |
| **ValueNetKind** | Enum selecting between `Mlp` and `Gnn` policy heads. |
