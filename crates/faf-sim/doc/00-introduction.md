# 0. Introduction: Why FAF Build Orders?

> Build orders are the opening chess moves of real-time strategy games. In FAF,
> they decide whether you have enough economy, technology, and units at the
> moment the game demands them.

## The dream

Imagine a Rust program that, given any FAF goal — a Monkeylord, a Fatboy, a
T3 air factory, or even a full game-winning composition — produces a near-
optimal build order. Not from a script, but by *learning* from experience in a
fast simulator.

This project explores that idea. It sits at the intersection of:

- **Game simulation:** a deterministic, high-fidelity model of FAF economy and
  construction (`faf-sim`).
- **Machine learning:** algorithms that improve from trial and error.
- **Rust:** a systems language that can run the simulator, the neural network,
  and the search all in one fast, reliable binary.

## Why FAF is a great testbed

1. **Clear objective.** Finish the goal as fast as possible, with the least
   wasted investment. That is a crisp optimization target.
2. **Rich but structured rules.** The tech graph, unit stats, and economy
   formulas are known. The challenge is sequencing, not physics.
3. **Human expertise exists.** Experienced players know good build orders. We
   can compare against them, learn from them, and maybe beat them.
4. **Deterministic simulator.** Unlike the full game, our simulator has no fog
   of war, no enemy, and no randomness. We can run millions of episodes to
   train an agent.

## Why Rust?

Rust gives us:

- **Speed:** training needs many simulator rollouts. Rust is fast enough to do
  them on a laptop.
- **Type safety:** the build graph and economy state have many invariants.
  Rust's type system helps catch mistakes at compile time.
- **Ecosystem:** libraries like `candle`, `burn`, and `dfdx` bring modern
  machine learning to Rust without leaving the language.

## What this book/project is not

- It is not a general introduction to machine learning. We focus on the
  techniques relevant to this problem.
- It is not a full FAF bot. We optimize build orders in isolation, not a
  complete 1v1 game with an opponent.
- It is not a guarantee of beating pro players. The goal is to learn and to
  build something interesting.

## What we will build, step by step

1. Understand the existing simulator (`model.md`).
2. See why classical search (beam search, heuristics) has limits.
3. Learn the machine-learning tools that can help.
4. Frame FAF as a learning environment.
5. Pick a concrete first approach and implement it in Rust.
6. Measure, iterate, and document what we discover.

## A note for beginners

Machine learning has a lot of jargon. We will introduce it slowly. Every time
you see an acronym like **PPO**, **MCTS**, or **GNN**, we will explain what it
means and why someone might use it for FAF. By the end, the vocabulary will
feel natural because each term will be tied to something concrete.

Let's start with the optimization problem itself.
