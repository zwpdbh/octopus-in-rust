# 0. Why RL for Build Orders?

A FAF build order is a sequence of build/upgrade/assist/wait decisions that grows your economy and technology until a goal is reached. The objective is simple: finish the goal as fast as possible. Finding the optimal sequence is not simple.

This tutorial explains why **reinforcement learning (RL)** is a good fit for FAF build-order optimization, why we train the policy with REINFORCE, and why we use **Monte Carlo Tree Search (MCTS)** only during simulation/inference. The implementation is in Rust with the [`Burn`](https://github.com/tracel-ai/burn) deep-learning framework.

## The search space is enormous and sequential

At any moment you may have many legal next units. Over a typical build order you make dozens of interdependent decisions. A small change early on — one extra engineer, one delayed power generator — can change the completion time by minutes later.

Classical search algorithms struggle with this:

- **Greedy search** picks the single best-looking next state at every step. It is fast but myopic: it cannot sacrifice short-term economy for a faster tech path later.
- **Beam search** keeps the top `K` states at each layer. It sees a little further, but the beam is still shallow compared with the full horizon, and its scoring heuristic is hand-written.

Both are **open-loop**: they generate a plan up front and hope the simulator follows it. If rounding, stall, or future choices make the real state drift, the plan has no way to recover until the next full replan.

## Why Burn?

Burn lets us keep the simulator, the policy network, and the training loop in a single Rust workspace:

- `burn::module::Module` gives us a typed, differentiable neural network that can be recorded to disk and loaded back.
- `burn::optim` provides Adam and other optimizers.
- `Autodiff<NdArray>` gives us gradients on the CPU without leaving Rust.

Because the simulator is written in the same language as the model, we can run millions of deterministic simulation steps during training and backpropagate through the policy that produced them.

## What MCTS adds (during simulation)

MCTS is used only when you run `faf-sim simulate`, not during training. It is **closed-loop**. It keeps a search tree rooted in the current observed state, expands the most promising branches, and recomputes the best action from the real state each tick. Small deviations do not compound because the planner always reasons from the latest state.

MCTS also naturally balances two things that are hard to encode by hand:

1. **Exploration** — trying actions whose short-term value is unclear.
2. **Exploitation** — doubling down on actions that look good in simulation.

The UCT formula (covered in [chapter 8](07-mcts-search.md)) does this balance mathematically. During training, the policy explores by sampling from its own output with epsilon-greedy noise instead of building a tree.

## Why a learned policy, not random rollouts?

In classic MCTS, a leaf is evaluated by playing random moves to the end and averaging the result. For FAF this is too expensive:

- The horizon is long (minutes of game time, many build steps).
- The reward is sparse (you only know the result when the goal is reached).
- Random rollouts produce mostly terrible build orders, so the signal is noisy.

A **learned hierarchical policy** replaces the random rollout. It is a single `burn::module::Module` that, in a small number of forward passes, decides:

1. Whether to upgrade a factory (`None`, `T1→T2`, or `T2→T3`).
2. Which strategic direction to focus on (`Mass`, `Energy`, `BuildPower`, or `Goal`).
3. Which concrete plan-graph edge to satisfy next inside that direction.
4. How much build power to allocate to that edge.
5. Which `[T1, T2, T3]` engineers should provide that build power.

During simulation MCTS still explores the tree, but it selects concrete edges with the policy instead of simulating random sequences to the end. During training the policy is sampled directly, without any tree.

The combination looks like this:

```text
        current SimulationState
                │
                ▼
        ┌─────────────────────────────────┐
        │  HierarchicalPolicyNet          │
        │  • upgrade head                 │
        │  • direction head               │
        │  • action head                  │
        │  • power head                   │
        │  • squad head                   │
        └────────────────┬────────────────┘
                         │
                         ▼
                best SimAction
                         │
                         ▼
                simulator tick
                         │
                         ▼
                next SimulationState
```

## Why MCTS if the policy can act alone?

A natural question is: if `macro_policy_plan` already turns the network output into a concrete `SimAction`, why do we need MCTS at all?

The answer is that MCTS is **not required** to use the policy. The policy can act by itself in a single forward pass:

```text
state → HierarchicalPolicyNet → upgrade/direction/edge/power/squad → SimAction
```

That one-step mode is exactly what the trainer uses, because running MCTS inside every training episode would be too slow.

MCTS becomes useful **after** training, during simulation. It wraps the same one-step policy and uses it to look ahead:

```text
Planner::plan(state, goal)
    │
    ▼
MCTS tree search
    │  • each node expands a legal plan-graph edge
    │  • selection uses network priors (PUCT)
    │  • leaf values come from greedy policy rollouts
    │  • many futures are simulated and averaged
    ▼
action with highest visit count
```

So the relationship is:

- **Training:** REINFORCE on one-step policy rollouts. No MCTS.
- **Simulation:** MCTS searches through many one-step policy rollouts to pick a more robust action.

MCTS adds lookahead and averaging. It can find actions that look slightly worse now but lead to much better states later, and it can recover when the network's single-step greedy choice is wrong.

## Why FAF is a good testbed for this

- **Deterministic simulator.** The same state and action always produce the same next state, so MCTS rollouts are exact and reproducible.
- **Known rules.** Unit stats, build powers, tech requirements, and upgrade costs come from the `Units` repository; there is no hidden physics.
- **Clear objective.** Minimize completion time, with a secondary efficiency metric.
- **Compact state.** `SimulationState` is structured data, not raw pixels, so featurization is straightforward.

## What this tutorial is not

- It is not a general machine-learning textbook. We assume you know what a neural network and a loss function are.
- It is not a full FAF bot. We optimize build orders in isolation, with no opponent and no fog of war.
- It is not a guarantee of pro-level play. The goal is to build a strong, interpretable planner and learn from the process.

## The roadmap in one paragraph

We model the simulator state as an MCTS node, generate legal candidates from the `PlanGraph`, build a small hierarchical `HierarchicalPolicyNet` in Burn to map state features to a factory-upgrade decision, a direction, a concrete edge, build power, and engineer squad, resolve each decision into a concrete command, train the network with REINFORCE, and run UCT search on top of the trained network during simulation. The rest of this tutorial walks through each piece.
