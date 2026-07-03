# Glossary

| Term | Meaning |
| --- | --- |
| **Action head** | Learned Burn head that scores concrete plan-graph edges inside a chosen direction. |
| **Action space** | All legal plan-graph edges from the current `SimulationState`, plus the wait action. |
| **Adjacency bonus** | Reduced energy/mass cost or build time when structures are placed near each other. Encoded in `Units` and applied to build projects. |
| **Backend** | The Burn compute backend. The `faf-sim` library defaults to CPU (`NdArray`); the `faf-sim-cli` package defaults to CUDA for training. Cross-platform GPU training can also use `Wgpu`. Aliased as `TrainBackend` in the training modules. |
| **Baseline** | Moving average of recent episode returns used to center REINFORCE advantages and reduce variance. |
| **Build order** | A sequence of construction actions that takes the economy from the starting commander to a target unit. |
| **BuildGraph** | Dynamic graph inside `SimulationState` that records the actual units and projects in the current game. It starts with the ACU and grows as the simulator executes actions. Not to be confused with the static `PlanGraph`. |
| **BuildPower** | Total construction rate contributed by idle engineers, measured in build points per second. |
| **Burn** | The Rust deep-learning framework used by `faf-sim` for training and inference. |
| **SimulationMsg** | An actor message sent from the planner to the simulator. It tells the simulator to execute a build, upgrade, assist, or goal action with a specific squad of builders. |
| **C_puct** | UCT exploration constant controlling the trade-off between exploitation and exploration. |
| **Deterministic policy** | At inference time, always picks the highest-scoring macro edge and rounds power/squad values. Used for evaluation. |
| **Direction head** | Learned Burn head that picks a strategic focus: `IncreaseMass`, `IncreaseEnergy`, `IncreaseBP`, or `Goal`. |
| **EdgeCategory** | Strategic focus tag (`IncreaseMass`, `IncreaseEnergy`, `IncreaseBP`, `Goal`) attached to every plan-graph edge. |
| **Episode** | One full rollout from the starting state until the goal is reached or `max_steps` is exceeded. |
| **Faction** | One of UEF, Cybran, Aeon, or Seraphim. Determines available units and build trees. |
| **Feature vector** | Fixed-size numerical representation of a `SimulationState` fed into the learned direction network. |
| **Fine-tuning** | Supervised training on the best trajectory found during REINFORCE, run after the main loop. |
| **SimulationState** | The authoritative discrete simulator state: units, projects, economy, completed structures. |
| **GNN** | Graph Neural Network. A planned alternative value head that reasons directly over the plan graph. Not yet implemented. |
| **Goal** | The abstract target the planner is trying to reach: `{ tech_level, mass_cost, energy_cost, build_time }`. The CLI resolves a concrete unit such as `novaxcenter` into a `Goal`. |
| **HierarchicalPolicyNet** | The single Burn `Module` that implements the five-head policy network. |
| **MCTS** | Monte Carlo Tree Search. Uses UCT selection, expansion, rollout, and backup to choose actions. |
| **Module** | Burn derive macro that makes a struct recordable, loadable, optimizable, and device-movable. |
| **NodeId** | Opaque identifier for a node in `SimulationState` (a unit instance or project). |
| **Observation** | Actor message carrying either a `BuildEvent` or the full `SimulationState` from the simulator to the decision actor. |
| **PlanEdge** | A typed, stable action candidate with source, target, kind, and strategic category. |
| **PlanEdgeIndex** | Stable ordered list of `PlanEdge`s derived from a `PlanGraph`, used to index network outputs. |
| **PlanGraph** | Static directed acyclic graph derived from `Units` and the `Goal`. It catalogues every legal build/upgrade edge and is used to enumerate legal actions and to mask network outputs. It does not change during an episode or rollout. See also `BuildGraph`. |
| **Planner** | The public facade that owns the strategy, configuration, and policy bundle. |
| **Policy bundle** | The learned hierarchical network saved and loaded together; macro alias for `HierarchicalPolicyNet`. |
| **Power head** | Learned Burn head that outputs a scalar target build power for the selected edge. |
| **Rollout** | In MCTS, a full simulation from a leaf node using the greedy policy network on a cloned `SimulationState`. The rollout adds nodes to the cloned build graph through normal simulator execution and returns a scalar value estimate. |
| **REINFORCE** | Policy-gradient algorithm used to train the four heads from Monte Carlo returns. |
| **Patience** | Number of episodes without a new best completion time after which training stops early. |
| **PUCT** | UCT variant that incorporates a learned prior probability into the exploration bonus. |
| **Return** | Discounted sum of rewards over an episode, used as the target for REINFORCE. |
| **Selection** | In MCTS, traversing the tree from the root to a leaf using the UCB1/PUCT formula. |
| **Shortfall** | Historical feedback channel for unmet engineer demand. Removed in the current direction-only design. |
| **SimAction** | Internal representation of a simulator action: `Build`, `Upgrade`, `Assist`, or `Wait`. |
| **Simulator tick** | One fixed-duration step of the discrete-time economy and build-progress simulation. |
| **Squad head** | Learned Burn head that outputs desired `[T1, T2, T3]` engineer counts. |
| **Upgrade head** | Learned Burn head that outputs factory-upgrade options: `NoUpgrade`, `T1→T2`, or `T2→T3`. |
| **Strategy** | The planner enum. Currently only `Strategy::Mcts` exists, with `ValueNetKind::Mlp` or `Gnn`. |
| **UCT** | Upper Confidence Bound applied to Trees. The selection formula used by MCTS. |
| **UnitKind** | Type-level identifier for a unit class, e.g. `UnitKind::Engineer(T3)` or `UnitKind::Unique(UnitId("UEL0301".to_string()))`. |
| **Units** | The static unit database: stats, build lists, prerequisites, adjacency bonuses. |
| **Upgrade** | Replacing an existing structure with a higher-tier version, such as T1 mex → T2 mex. |
| **ValueNetKind** | Enum selecting between `Mlp` and `Gnn` policy heads. |
