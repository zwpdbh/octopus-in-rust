# 3. Reinforcement Learning for Beginners

> Reinforcement learning is learning by doing. An agent takes actions in an
> environment, receives rewards, and learns to choose better actions over time.

This chapter introduces the core ideas of RL and explains the algorithms most
relevant to FAF build orders. We expand every acronym and connect each idea to
our concrete problem.

## The RL loop

At each step:

1. The agent observes the **state** `s` (e.g., the current `GraphState`).
2. The agent picks an **action** `a` (e.g., build a unit, assist a project, wait).
3. The environment transitions to a new state `s'` and gives a **reward** `r`.
4. The agent updates its strategy based on `(s, a, r, s')`.

An **episode** is one full run: from the starting ACU until the goal is reached
or a time limit is hit.

## Policy and value function

Two central concepts:

- **Policy** — the agent's strategy. It answers: "Given this state, what action
  should I take?" A policy can be a lookup table, a neural network, or a search
  algorithm.
- **Value function** — an estimate of how good a state is. It answers: "From
  this state, how much total reward can I expect?"

For FAF:

- A **policy** might say: "Given these idle builders and this economy, build a
  T1 pgen."
- A **value function** might say: "This state looks like it will finish the
  Monkeylord in 12 minutes."

## Exploration vs. exploitation

The agent must balance:

- **Exploitation:** do what currently seems best.
- **Exploration:** try something new to see if it is better.

If the agent always exploits, it may get stuck in a local optimum. If it always
explores, it never commits to a good plan. RL algorithms use schemes like
**epsilon-greedy** (random action with small probability) or entropy bonuses to
manage this trade-off.

## Model-free vs. model-based

- **Model-free RL** learns directly from experience without knowing the
  environment's rules. It is simpler but may need more data.
- **Model-based RL** learns or uses a model of the environment to plan ahead.
  Because we already have a perfect FAF simulator, model-based methods are very
  attractive.

## Common RL algorithms

### DQN — Deep Q-Network

DQN learns a **Q-function**: the expected total reward of taking action `a` in
state `s` and then acting optimally. The agent picks the action with the
highest Q-value.

- Good for discrete action spaces.
- Works best when rewards are frequent.
- **FAF relevance:** Could work for small action spaces, but build orders have
  sparse rewards and a huge action space, so DQN alone may struggle.

### A2C — Advantage Actor-Critic

A2C keeps two things:

1. An **actor** — the policy that chooses actions.
2. A **critic** — the value function that evaluates states.

The **advantage** is the difference between the actual reward and what the
critic expected. The actor is updated to favor actions with positive advantage.

- Stable and relatively simple.
- **FAF relevance:** A good baseline. The actor proposes build/assist actions;
  the critic estimates completion time.

### PPO — Proximal Policy Optimization

PPO is a newer algorithm that improves on A2C. It updates the policy carefully
so that one bad training step does not change the policy too much. "Proximal"
means "nearby" — it keeps the new policy close to the old one.

- Very popular; works well in many domains.
- Easier to tune than many alternatives.
- **FAF relevance:** A strong candidate for training a build-order policy.

### MCTS — Monte Carlo Tree Search

MCTS is not a neural-network algorithm by itself. It is a search method that
explores possible futures by simulating random or guided rollouts and building
a tree of decisions. AlphaGo and AlphaZero combined MCTS with a neural network.

In AlphaZero-style systems:

- A neural network predicts both the best move (policy) and the expected
  outcome (value).
- MCTS uses those predictions to focus search on promising branches.
- The search results are then used to improve the network.

- **FAF relevance:** Very attractive because our simulator is deterministic and
  fast. We can use MCTS to plan build orders at decision time, guided by a
  learned value network.

## Algorithms compared

| Algorithm | What it learns | Data needed | Strengths | Weaknesses |
|-----------|----------------|-------------|-----------|------------|
| DQN | Q-values | Episodes | Simple | Struggles with large/sparse action spaces |
| A2C | Policy + value | Episodes | Stable, well-understood | Can be sample-inefficient |
| PPO | Policy + value | Episodes | Robust, widely used | Needs many episodes |
| MCTS + value net | Value + search guidance | Self-play / rollouts | Powerful planning | More complex system |

## Reward shaping

In FAF, the only true reward is at the end: `-completion_time`. This is a
**sparse reward**. Sparse rewards make learning slow because the agent gets
little feedback early in training.

**Reward shaping** adds small intermediate rewards to guide learning:

- Positive reward for completing a prerequisite unit.
- Negative reward for energy stall.
- Positive reward for increasing mass income.

But shaping can backfire. The agent may optimize the shaped reward instead of
the true objective. For example, it might over-build economy to collect income
rewards and never finish the goal.

## Key takeaways

- RL is learning from interaction with an environment.
- A policy chooses actions; a value function evaluates states.
- PPO and A2C are solid model-free choices; MCTS + value network is powerful
  when you have a fast simulator.
- Reward design is tricky. Sparse rewards are honest but slow; shaped rewards
  are faster but can mislead.

Next, we map these abstract ideas onto the concrete FAF simulator.
