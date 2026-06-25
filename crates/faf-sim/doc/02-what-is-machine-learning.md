# 2. What Is Machine Learning?

> Machine learning is programming with examples. Instead of writing a rule for
> every situation, you write a program that learns a rule from data.

This chapter is a gentle primer. If you already know what a neural network is,
you can skim it, but we introduce vocabulary that the rest of the book uses.

## The three main flavors

### 1. Supervised learning

You give the model pairs of `(input, expected output)`. The model learns to
map inputs to outputs.

- **Example:** predict the final completion time of a FAF build order given the
  current `GraphState`.
- **Input:** a snapshot of units, economy, and active projects.
- **Expected output:** the number of seconds until the goal finishes.

### 2. Unsupervised learning

You give the model data without expected outputs. It tries to find structure,
such as clusters or patterns.

- **Example:** group similar FAF opening strategies together.
- This is less central to our project, but useful for analysis.

### 3. Reinforcement learning (RL)

The model — called an **agent** — interacts with an **environment**. The
environment gives the agent a **reward** after each action. The agent learns to
pick actions that maximize total reward over time.

- **Example:** the agent chooses which unit to build next in FAF. The
  environment is the simulator. The reward is better when the goal finishes
  faster.
- RL is the main focus of this project because build orders are a sequence of
decisions.

## Model, parameters, and training

A **model** is a function with adjustable internal values called
**parameters** or **weights**. At first the parameters are random, so the model
is useless. **Training** is the process of adjusting the parameters so the
model produces better outputs.

Think of parameters like knobs on a sound mixer. Training is an automatic
procedure that turns the knobs until the output sounds right.

## Loss function

A **loss function** measures how wrong the model is. Training tries to minimize
loss.

- For supervised completion-time prediction: loss could be
  `(predicted_time - actual_time)^2`.
- For RL: loss is derived from how much reward the agent collected.

## Neural networks

A **neural network** is a particular kind of model made of layers of simple
computing units. It is good at learning complex patterns from raw inputs.

You do not need to understand every detail to use one. The important intuition
is:

- Input goes in one side (e.g., a `GraphState` encoded as numbers).
- Many simple computations happen in hidden layers.
- Output comes out the other side (e.g., a score or an action preference).

Different shapes of networks are good for different inputs:

| Name | Good for | FAF relevance |
|------|----------|---------------|
| **MLP** (multi-layer perceptron) | Fixed-size vectors | Simple state summaries. |
| **CNN** (convolutional neural network) | Images or grids | Not obvious for FAF; could encode a tech grid. |
| **RNN / LSTM** | Sequences over time | Build-order history. |
| **Transformer** | Long sequences, attention | Complex planning sequences. |
| **GNN** (graph neural network) | Graphs with nodes and edges | The FAF build graph! |

## Training data

Where does the data come from?

- **Supervised:** you need examples with known answers. For FAF, that might mean
  running many build orders and recording the final completion time of each
  state.
- **RL:** the agent generates its own data by trying things in the simulator.

## Rust ML libraries

Several Rust libraries can build and train neural networks:

- **`candle`** — lightweight, Hugging Face-backed, good for inference and
  training small-to-medium models.
- **`burn`** — a deep learning framework with a flexible backend system.
- **`dfdx`** — type-safe deep learning with compile-time shape checking.

For this project, any of them could work. The concepts are more important than
the exact library.

## Key takeaways

- Machine learning lets a program learn from data instead of explicit rules.
- Reinforcement learning is the right family for sequential decisions like
  build orders.
- Neural networks are flexible function approximators; their architecture
  should match the data (graphs for build graphs).

Next, we dive into reinforcement learning specifically.
