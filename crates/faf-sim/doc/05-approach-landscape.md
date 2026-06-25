# 5. Approach Landscape

There are many ways to apply machine learning to FAF build orders. This
chapter surveys the landscape, explains each approach in plain language, and
suggests where to start.

## 1. Pure reinforcement learning

Train a neural network policy end-to-end. The network takes a `GraphState` and
outputs the next action. PPO or A2C would be the natural algorithm choices.

**Pros:**
- Can discover unusual strategies humans might miss.
- Single clean system.

**Cons:**
- Needs a huge number of episodes.
- Action space is large and combinatorial.
- Sparse rewards make early learning slow.
- Hard to debug when the agent does something weird.

**Verdict:** Possible, but probably not the easiest first step.

## 2. Learned value function + classical search

Keep the existing beam search, but replace the hand-written heuristic with a
neural network that estimates remaining completion time.

The training loop is supervised:

1. Generate many build orders by running the current planner or random
   policies.
2. For many observed `GraphState`s, record the true final completion time.
3. Train a neural network to predict that time.
4. Use the network inside beam search to rank states.

**Pros:**
- Builds directly on the existing planner.
- Constraints are still enforced by the simulator.
- Easier to interpret: the network is just a better `heuristic::score`.

**Cons:**
- The search still dominates runtime.
- The network only sees states from the data distribution.

**Verdict:** A strong first milestone. Low risk and clear improvement path.

## 3. Imitation learning

Record good build orders — from the beam planner, from human players, or from
analyzed replays — and train a policy to imitate them.

**Pros:**
- Avoids cold-start exploration.
- Produces a reasonable policy quickly.

**Cons:**
- Cannot exceed the quality of the expert it imitates.
- May fail in states the expert never visited.

**Verdict:** Useful as a pre-training step before RL fine-tuning.

## 4. Macro actions

Instead of learning every low-level build decision, define high-level actions:

- `ExpandEconomy`
- `TechUp`
- `BuildEngineers`
- `BuildGoal`

A small planner translates each macro action into concrete simulator commands.
The learning agent only chooses the sequence of macros.

**Pros:**
- Smaller action space.
- Easier credit assignment.
- More interpretable strategies.

**Cons:**
- The macro definitions encode human bias.
- May miss clever micro-level timings.

**Verdict:** A good compromise between pure RL and classical search.

## 5. MCTS with a learned value network

Use **Monte Carlo Tree Search** at decision time. A neural network provides:

- A **policy prior** suggesting which actions are worth exploring.
- A **value estimate** for leaf states.

The simulator rolls out candidate futures. Because FAF is deterministic, the
rollouts are exact. MCTS focuses computation on the most promising branches.

**Pros:**
- Combines learning with explicit planning.
- Can beat policies that do not plan ahead.

**Cons:**
- More complex system.
- Needs a fast simulator and a well-trained value network.

**Verdict:** The most powerful long-term approach for FAF. The project has
selected this as the primary direction; see [`06-mcts-value-net-plan.md`](./06-mcts-value-net-plan.md)
for the concrete implementation plan.

## 6. Curriculum learning

Start with trivial goals and gradually increase difficulty:

1. Build a T1 pgen.
2. Build a T1 factory + engineer.
3. Build a T2 factory.
4. Build a T3 engineer.
5. Build a Monkeylord.

The agent masters simple episodes before facing hard ones.

**Pros:**
- Faster learning.
- Reduces sparse-reward problems early on.

**Cons:**
- Requires designing a curriculum.
- The agent may overfit to early goals.

**Verdict:** Almost certainly useful, regardless of the base algorithm.

## Recommended starting path

For this project, the selected sequence is detailed in
[`06-mcts-value-net-plan.md`](./06-mcts-value-net-plan.md). The high-level
milestones are:

1. **Milestone 0 — Baseline and instrumentation.** Lock down beam-search
   benchmarks and trajectory logging.

2. **Milestone 1 — State featurization.** Convert `GraphState` into a fixed-size
   feature vector the network can consume.

3. **Milestone 2 — Learned value function.** Train a network to predict
   remaining completion time from supervised rollout data.

4. **Milestone 3 — MCTS with value net.** Replace beam search with closed-loop
   UCT search guided by the value network.

5. **Milestone 4 — Policy prior.** Add a policy network head to guide MCTS
   exploration, AlphaZero-style.

6. **Stretch goal — End-to-end RL.** Once the components work, explore whether
   a pure PPO agent can match or exceed the hybrid system.

This path keeps the project grounded in the existing simulator while steadily
increasing the role of learning.

## What to read next

The next chapters — when they are written — will cover:

- Encoding a variable-size `GraphState` for a neural network.
- Action masking for valid move generation.
- Reward shaping without losing sight of the true objective.
- Implementing a training loop in Rust.
- Running experiments and comparing against the beam-search baseline.

For now, the foundation is in place: we know the problem, the ML concepts, and
the high-level strategy. The real work begins with code.
