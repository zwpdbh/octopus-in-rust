# 0. Why MCTS for FAF Build Orders?

A FAF build order is a sequence of build/upgrade/assist/wait decisions that grows your economy and technology until a goal unit is finished. The objective is simple: finish the goal as fast as possible. Finding the optimal sequence is not simple.

This chapter explains why **Monte Carlo Tree Search (MCTS)** guided by a **learned policy network** is a good fit, and why the existing greedy and beam strategies hit a ceiling.

## The search space is enormous and sequential

At any moment you may have many legal next units. Over a typical build order you make dozens of interdependent decisions. A small change early on — one extra engineer, one delayed pgen — can change the completion time by minutes later.

Classical search algorithms struggle with this:

- **Greedy search** picks the single best-looking next state at every step. It is fast but myopic: it cannot sacrifice short-term economy for a faster tech path later.
- **Beam search** keeps the top `K` states at each layer. It sees a little further, but the beam is still shallow compared with the full horizon, and its scoring heuristic is hand-written.

Both are **open-loop**: they generate a plan up front and hope the simulator follows it. If rounding, stall, or future choices make the real state drift, the plan has no way to recover until the next full replan.

## What MCTS adds

MCTS is **closed-loop**. It keeps a search tree rooted in the current observed state, expands the most promising branches, and recomputes the best action from the real state each tick. Small deviations do not compound because the planner always reasons from the latest state.

MCTS also naturally balances two things that are hard to encode by hand:

1. **Exploration** — trying actions whose short-term value is unclear.
2. **Exploitation** — doubling down on actions that look good in simulation.

The UCT formula (covered in [`04-mcts-search.md`](./04-mcts-search.md)) does this balance mathematically.

## Why a learned policy, not random rollouts?

In classic MCTS, a leaf is evaluated by playing random moves to the end and averaging the result. For FAF this is too expensive:

- The horizon is long (minutes of game time, many build steps).
- The reward is sparse (you only know the result when the goal finishes).
- Random rollouts produce mostly terrible build orders, so the signal is noisy.

A **learned policy network** replaces the random rollout. It is a function `π(action | state)` that, in a single forward pass, scores every legal candidate action and samples the next move. MCTS still explores the tree, but it selects moves with the network instead of simulating random sequences to the end.

The combination looks like this:

```text
        current GraphState
                │
                ▼
        ┌───────────────┐
        │  MCTS search  │◄────── action preferences
        │  (UCT tree)   │        from policy net
        └───────┬───────┘
                │
                ▼
        best SimAction
                │
                ▼
        simulator tick
                │
                ▼
        next GraphState
```

The current implementation uses the policy network as a one-step stochastic planner. Full UCT tree search will be layered on top later while reusing the same network.

## Why FAF is a good testbed for this

- **Deterministic simulator.** The same state and action always produce the same next state, so MCTS rollouts are exact and reproducible.
- **Known rules.** Unit stats, build powers, tech requirements, and upgrade costs come from the `Units` repository; there is no hidden physics.
- **Clear objective.** Minimize completion time, with a secondary efficiency metric.
- **Compact state.** `GraphState` is structured data, not raw pixels, so featurization is straightforward.

## What this track is not

- It is not a general machine-learning textbook. We assume you know what a neural network and a loss function are.
- It is not a full FAF bot. We optimize build orders in isolation, with no opponent and no fog of war.
- It is not a guarantee of pro-level play. The goal is to build a strong, interpretable planner and learn from the process.

## The roadmap in one paragraph

We model the simulator state as an MCTS node, generate legal candidates from the `PlanGraph`, train a small network to score `(state, candidate)` pairs, run UCT search at every decision, and use the resulting planner inside `faf-sim`. The rest of this track walks through each piece.
