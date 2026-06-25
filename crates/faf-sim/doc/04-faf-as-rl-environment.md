# 4. FAF as an RL Environment

This chapter translates the FAF simulator into the language of reinforcement
learning. By the end, you should see exactly how each RL concept maps to a
concrete Rust type or operation.

## Environment = the simulator

The **environment** is everything outside the agent. In our case, it is the
`faf-sim` simulator, especially `sim::GraphState` and its `tick` method.

When the agent chooses an action, the environment:

1. Applies the action (starts a project, assists a project, or waits).
2. Advances time by `dt`.
3. Updates the economy, completes units, and returns the new state.

The environment is **deterministic**: the same state and action always produce
the same next state. That is a huge advantage for learning.

## State = GraphState

The **state** is a snapshot of the game. In Rust terms:

```rust
// docref: example
pub struct FafObservation {
    pub time: f64,
    pub graph: BuildGraph,
    pub economy: EconomyState,
    pub active_projects: Vec<OngoingBuild>,
    pub goal_unit_id: String,
}
```

This is essentially a view of `sim::GraphState` plus the goal. The challenge is
that the state is variable-size: the number of units and projects changes.
Later we will discuss how to encode this for a neural network.

## Action = Build, Assist, or Wait

The **action space** mirrors the existing search actions in
`planner::search::SearchAction`:

```rust
// docref: example
pub enum FafAction {
    Build {
        unit_id: String,
        builders: Vec<NodeId>,
    },
    Assist {
        project_node: NodeId,
        builders: Vec<NodeId>,
    },
    Wait,
}
```

Not every action is legal in every state. For example, you cannot assign a
busy builder to a new project, and you cannot build a T3 engineer with a T1
factory. The environment must reject illegal actions, or — better — the agent
must be told which actions are legal through **action masking**.

## Reward = negative completion time

The simplest reward is given only at the end of the episode:

```text
reward = -completion_time
```

Finishing faster gives a higher (less negative) reward. This is honest but
sparse.

Alternative reward structures to experiment with:

- `-1` per step: encourages finishing quickly without caring about the exact
  time.
- Small bonuses for completing prerequisite units.
- Penalties for energy stall or idle builders.
- A terminal bonus tied to economy efficiency.

Each alternative changes what the agent learns, so we will measure carefully.

## Episode = one build order

An episode begins with the starting ACU and ends when:

- The goal unit is completed.
- A maximum time limit is reached.
- The state becomes stuck (no progress possible).

During training, the agent plays many episodes. Each episode is a complete
build order.

## Transition = (state, action, reward, next_state)

Every tick produces a **transition**. The agent stores transitions in a buffer
and uses them to update its policy or value function.

```rust
// docref: example
pub struct Transition {
    pub state: FafObservation,
    pub action: FafAction,
    pub reward: f64,
    pub next_state: FafObservation,
    pub done: bool,
}
```

## Determinism and reset

Because the simulator is deterministic, we can **reset** an episode to the same
initial state every time. This lets us compare policies fairly: run the same
initial conditions and see who finishes faster.

We can also create a **curriculum** of goals:

1. Easy: build a T1 pgen.
2. Medium: build a T1 factory and an engineer.
3. Hard: build a T3 engineer.
4. Very hard: build a Monkeylord.

Training on easier goals first helps the agent learn before tackling the
hardest tasks.

## Why this is convenient

Many RL benchmarks are messy: stochastic physics, noisy sensors, delayed
rewards, high-dimensional pixels. FAF build orders are clean by comparison:

- Deterministic transitions.
- Exact rules from `faf-units` and `faf-sim`.
- Compact, structured state.
- Clear goal.

That cleanliness does not make the problem easy, but it makes experiments
easier to interpret. We can focus on learning algorithms, not on wrangling
noisy simulators.

## Key takeaways

- `GraphState` is the RL state.
- `Build`/`Assist`/`Wait` are the RL actions.
- `-completion_time` is the natural reward.
- Determinism and reset make FAF a friendly RL environment.

Now that we know how to frame the problem, let's survey the practical
approaches we could take.
