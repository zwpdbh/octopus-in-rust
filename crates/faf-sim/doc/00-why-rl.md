# 0. Why RL for Build Orders?

A FAF build order is a sequence of build/upgrade/assist/wait decisions that grows your economy and technology until a goal is reached. The objective is simple: finish the goal as fast as possible. Finding the optimal sequence is not simple.

This tutorial explains why **reinforcement learning (RL)** is a good fit for FAF build-order optimization and why we train a single direction policy with REINFORCE. The implementation is in Rust with the [Burn](https://github.com/tracel-ai/burn) deep-learning framework.

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
- `Autodiff<NdArray>` gives us gradients on the CPU without leaving Rust, while `Autodiff<Cuda>` or `Autodiff<Wgpu>` can train on a GPU.

Because the simulator is written in the same language as the model, we can run millions of deterministic simulation steps during training and backpropagate through the policy that produced them.

## What the policy learns

We do not ask the network to output every low-level build command. Instead, it learns the high-level strategic decisions that are hard to write by hand:

- Should we expand mass income now?
- Should we build more energy?
- Should we invest in build power (engineers)?
- Should we add energy storage?
- Should we start the goal?
- Should we upgrade a factory?

The network outputs a probability distribution over **six directions**. A deterministic heuristic layer then turns the chosen direction into a concrete build or upgrade action. This split keeps the learned part small and interpretable while moving concrete target selection into cheap, verifiable rules.

The overall flow is:

```text
        current SimulationState
                │
                ▼
        state_features(state)
                │
                ▼
        ┌───────────────────────┐
        │  HierarchicalPolicyNet │   ← Burn Module
        │  • shared backbone     │
        │  • direction head (6)  │
        └───────────┬───────────┘
                    │
                    ▼
            EdgeCategory direction
                    │
                    ▼
        heuristic::direction_to_action
                    │
                    ▼
            concrete SimAction
                    │
                    ▼
            simulator tick
                    │
                    ▼
            next SimulationState
```

## Why a learned policy, not hand-written heuristics?

Hand-written build orders work for one target on one map, but they are brittle:

- A small balance patch can change the optimal order.
- Different factions and targets need different orders.
- It is hard to weigh trade-offs like "one more mex now vs. one more engineer now" in closed form.

A learned policy discovers these trade-offs from experience. It is trained to maximize a single scalar — negative completion time — and generalizes across the states it has seen during training.

During training the policy explores by sampling from its own output with epsilon-greedy noise. During simulation we typically use the greedy argmax direction, which is deterministic and fast.

## Why one-step policy instead of search?

A natural question is: why not wrap the policy in a lookahead search? The answer is practical:

- The policy was trained with **REINFORCE on one-step rollouts**, so it was never trained to provide accurate value estimates for hypothetical future states.
- The simulator is deterministic, so replanning every tick from the real state already corrects drift.
- One forward pass per tick is fast enough for the reactive simulation loop.

So the relationship is:

- **Training:** REINFORCE on one-step policy rollouts.
- **Simulation:** one-step greedy (or sampled) policy evaluation per tick.

The policy itself is the planner. If a direction looks slightly worse now but leads to much better states later, the policy must learn that from the return signal during training.

## Why FAF is a good testbed for this

- **Deterministic simulator.** The same state and action always produce the same next state, so rollouts are reproducible.
- **Known rules.** Unit stats, build powers, tech requirements, and upgrade costs come from the `Units` repository; there is no hidden physics.
- **Clear objective.** Minimize completion time, with a secondary efficiency metric.
- **Compact state.** `SimulationState` is structured data, not raw pixels, so featurization is straightforward.

## What this tutorial is not

- It is not a general machine-learning textbook. We assume you know what a neural network and a loss function are.
- It is not a full FAF bot. We optimize build orders in isolation, with no opponent and no fog of war.
- It is not a guarantee of pro-level play. The goal is to build a strong, interpretable planner and learn from the process.

## The roadmap in one paragraph

We model the simulator state as a graph, extract an 11-dimensional feature vector, build a small Burn `Module` that maps features to a distribution over six strategic directions, resolve each direction into a concrete command with a deterministic heuristic, and train the network with REINFORCE. The rest of this tutorial walks through each piece.
