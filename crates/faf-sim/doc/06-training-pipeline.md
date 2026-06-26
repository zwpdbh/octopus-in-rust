# 6. Training Pipeline

A value network is only useful if it has seen states like the ones MCTS will evaluate. This chapter describes how to generate training data, warm-start the network with supervised learning, and optionally close the loop with self-play.

## Phase 1: supervised warm-start

Before MCTS can search intelligently, it needs a value function. The fastest way to get one is supervised learning on rollout data.

### Generate data

Run the existing beam planner (or even greedy and random policies) on a curriculum of goals:

1. T1 pgen
2. T1 factory + engineer
3. T2 factory
4. T3 engineer
5. Monkeylord

For every visited `GraphState`, record:

- the state features (using `featurize(state, goal_id, &units)`),
- the goal unit id,
- the true final completion time from that state.

Because the simulator is deterministic, the completion time from any state is exact. You do not need to average over multiple rollouts.

### Compute targets

Normalize the target so the network sees numbers near `[-1, 0]`:

```text
target = -completion_time / TIME_SCALE
```

A state that is already at the goal has `completion_time = 0` and `target = 0`. A state that is ten minutes away with `TIME_SCALE = 600` has `target = -1`.

### Train

Train the value net with mean-squared error. See [`03-value-network.md`](./03-value-network.md) for the architecture and loop. Hold out 10–20% of the data for validation. If validation loss stops improving, stop training.

The result is a network that can estimate remaining time for states similar to those in the training corpus. It will not yet be good at states MCTS discovers during search, but it is good enough to bootstrap MCTS.

## Phase 2: MCTS data collection

Once the warm-started network is plugged into MCTS, run MCTS on the same curriculum. The search will visit states the beam planner never explored. Log those states and their true outcomes.

This creates a second, richer dataset:

- states from beam-like plans,
- states from MCTS's own exploration,
- states from random perturbations.

Retrain the network on the combined dataset. This usually improves generalization.

## Phase 3: policy prior (AlphaZero-style)

A pure value net still lets MCTS explore blindly at the root. You can add a **policy prior**: a second network head that predicts which actions are promising.

The policy network takes the same features as the value network and outputs a probability for each legal action. Because the action space is variable-size, use action masking:

1. Generate all legal actions with `SearchConfig::successors(&units, state, goal_id, goal_chain)`.
2. Map each action to a fixed index in an action vocabulary. `Upgrade` actions are indexed by `(old_node, target_unit_id)`; `Build` actions by `unit_id`; `Assist` by `project_node`; `Wait` is a single fixed index.
3. Set the logits of illegal actions to `-inf` before softmax.

The UCT formula then becomes:

```text
UCT = (child.total_value / child.visits)
      + c_puct * prior(child) * sqrt(parent.visits) / (1 + child.visits)
```

A good prior focuses search on promising branches and ignores obviously bad ones, reducing the effective branching factor.

## Phase 4: self-play loop

With both value and policy networks, you can run a self-play improvement loop:

1. Run MCTS with the current networks to generate games.
2. Store `(state, policy_target, value_target)` tuples:
   - `policy_target` is the visit distribution over root children.
   - `value_target` is the final outcome.
3. Train both networks on the new data.
4. Evaluate the new networks against the previous ones on the benchmark suite.
5. Keep the winner as the new baseline.

This is the AlphaZero recipe adapted to a single-player optimization problem. The main difference is that "winning" means finishing the goal faster, not defeating an opponent.

## Curriculum and data diversity

The network will overfit if all training data comes from one planner. To prevent this:

- Generate data from greedy, beam, and random policies.
- Vary the goal and starting conditions.
- Add small perturbations to `dt` and resource caps.
- Periodically retrain from scratch on the growing dataset, not just fine-tune.

## When to stop

Stop iterating when:

- MCTS with the current network beats the beam baseline on the benchmark suite.
- Validation loss has plateaued for several training rounds.
- Adding more self-play data no longer improves benchmark results.

At that point the system is ready for wider experimentation: harder goals, multiple goals, or integration into a larger bot.
