# Glossary

| Term | Meaning |
| --- | --- |
| **Action space** | The six high-level `EdgeCategory` directions the policy can choose from. |
| **Adjacency bonus** | Reduced energy/mass cost or build time when structures are placed near each other. Encoded in `Units` and applied to build projects. |
| **Backend** | The Burn compute backend. The `faf-sim` library defaults to CPU (`NdArray`); the `faf-sim-cli` package defaults to CUDA for training. Cross-platform GPU training can also use `Wgpu`. Aliased as `TrainBackend` in the training modules. |
| **Baseline** | Moving average of recent episode returns used to center REINFORCE advantages and reduce variance. |
| **Build order** | A sequence of construction actions that takes the economy from the starting commander to a target unit. |
| **BuildGraph** | Dynamic graph inside `SimulationState` that records the actual units and projects in the current game. It starts with the ACU and grows as the simulator executes actions. Not to be confused with the static `PlanGraph`. |
| **BuildPower** | Total construction rate contributed by idle engineers, measured in build points per second. |
| **Burn** | The Rust deep-learning framework used by `faf-sim` for training and inference. |
| **Direction head** | Learned Burn head that outputs logits over the six `EdgeCategory` directions. |
| **Direction mask** | Boolean vector of length 6 indicating which directions are legal in the current state. |
| **EdgeAction** | `Build` or `Upgrade`; describes how a plan-graph edge is executed. |
| **EdgeCategory** | Strategic focus tag (`IncreaseMass`, `IncreaseEnergy`, `IncreaseBP`, `IncreaseEnergyStorage`, `Goal`, `UpgradeTech`) attached to every plan-graph edge and output by the direction head. |
| **Episode** | One full rollout from the starting state until the goal is reached or `max_steps` is exceeded. |
| **Faction** | One of UEF, Cybran, Aeon, or Seraphim. Determines available units and build trees. |
| **Feature vector** | Fixed-size numerical representation of a `SimulationState` fed into the learned direction network. Currently 11 floats. |
| **Fine-tuning** | Supervised training on the best trajectory found during REINFORCE, run after the main loop. |
| **Heuristic layer** | Deterministic rules in `heuristic.rs` that convert a selected `EdgeCategory` direction into a concrete `SimAction`. |
| **HierarchicalPolicyNet** | The Burn `Module` that implements the direction-only policy: shared backbone plus a single direction head. |
| **Module** | Burn derive macro that makes a struct recordable, loadable, optimizable, and device-movable. |
| **NodeId** | Opaque identifier for a node in `SimulationState` (a unit instance or project). |
| **Observation** | Actor message carrying either a `BuildEvent` or the full `SimulationState` from the simulator to the decision actor. |
| **PlanGraph** | Static directed acyclic graph derived from `Units` and the `Goal`. It catalogues every legal build/upgrade edge and is used to enumerate legal directions and to validate heuristic choices. It does not change during an episode or rollout. See also `BuildGraph`. |
| **Planner** | The public facade that owns the strategy, configuration, and policy bundle. |
| **Policy bundle** | The learned direction-only network saved and loaded together; macro alias for `HierarchicalPolicyNet`. |
| **Rollout** | A full episode sampled from the current policy, used to collect trajectories for REINFORCE. |
| **REINFORCE** | Policy-gradient algorithm used to train the direction head from Monte Carlo returns. |
| **Patience** | Number of episodes without a new best completion time after which training stops early. |
| **Return** | Discounted sum of rewards over an episode, used as the target for REINFORCE. |
| **Shortfall** | Historical feedback channel for unmet engineer demand. Removed in the current direction-only design. |
| **SimAction** | Internal representation of a simulator action: `Build`, `Upgrade`, `Assist`, `BuildGoal`, or `Wait`. |
| **Simulator tick** | One fixed-duration step of the discrete-time economy and build-progress simulation. |
| **Strategy** | The planner enum. Currently only `Strategy::Policy` exists, with `ValueNetKind::Mlp` or `Gnn`. |
| **UnitKind** | Type-level identifier for a unit class, e.g. `UnitKind::Engineer(T3)` or `UnitKind::Unique(UnitId("UEL0301".to_string()))`. |
| **Units** | The static unit database: stats, build lists, prerequisites, adjacency bonuses. |
| **Upgrade** | Replacing an existing structure with a higher-tier version, such as T1 mex → T2 mex. |
| **ValueNetKind** | Enum selecting between `Mlp` and `Gnn` policy heads. Only `Mlp` is implemented. |
