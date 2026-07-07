# 5. Reward Shaping

A build-order episode can last thousands of simulator ticks, and the only event the ultimate user cares about is the goal finishing. For the first baseline we deliberately keep the reward signal extremely simple: the agent is rewarded only for increasing its net mass income. This removes confounding objectives and lets us measure whether the policy can learn a coherent economy expansion before we add goal-seeking terms.

## Mass-income-only reward

`compute_step_reward` is called after every successful action. It compares the net mass income before and after the action and returns the clamped, scaled delta:

```rust
// crates/faf-sim/src/planner/policy/train/reward.rs ~line 14 — compute_step_reward
pub(crate) fn compute_step_reward(
    prev_state: &SimulationState,
    next_state: &SimulationState,
    _units: &Units, // unused for now; kept for API symmetry
    config: &TrainConfig,
) -> f32 {
    let prev_mass = prev_state.economy.net_mass_income as f32;
    let next_mass = next_state.economy.net_mass_income as f32;
    let mass_delta = (next_mass - prev_mass).clamp(-30.0, 30.0);
    (mass_delta * config.reward_mass_income_coef).clamp(-10.0, 10.0)
}
```

- A positive delta means the chosen direction increased mass generation, so the policy is encouraged to repeat that direction in similar states.
- A negative delta means mass income dropped, so the policy is pushed away from that direction.
- Both the raw delta (`±30`) and the final reward (`±10`) are clamped to prevent any single step from overwhelming the optimizer.

This is a **minimal baseline**, not the final reward. It is deliberately agnostic about reaching the goal: a policy that builds mexes forever can score highly without ever constructing the target unit. That is acceptable for a first experiment; richer shaping (goal-completion bonus, tech milestones, build-power terms) will be added once we confirm the optimizer can learn from a single clean signal.

## Why remove terminal bonuses, milestones, and timeouts?

Earlier versions combined per-step economy rewards with:

- A large terminal bonus/penalty based on whether the goal was reached.
- One-time tech milestone bonuses (T2 factory, T3 factory, T3 engineer).
- A timeout penalty for hitting `max_steps`.

These terms have been removed for the baseline because:

1. **They obscure credit assignment.** A terminal bonus only tells the agent that the whole episode was good or bad; it does not say which of the thousands of individual decisions mattered.
2. **They introduce hyperparameters that interact nonlinearly.** Balancing milestone sizes against the terminal bonus and per-step rewards made tuning fragile.
3. **They are unnecessary for the first question.** Before asking "can the policy learn to reach the goal?" we want to ask "can the policy learn to grow the economy consistently?"

The timeout penalty is also gone. Hitting `max_steps` simply ends the episode; there is no extra negative reward. The only signal the optimizer sees is the mass-income delta collected along the way.

## Online per-step updates

Because the reward is defined for each step, the trainer applies a policy-gradient update immediately after every step rather than waiting for the episode to finish. The loss for one step is:

```text
loss = -log π(direction | state) * reward
```

A positive reward increases the probability of the selected direction; a negative reward decreases it. There are no discounted returns, no return standardization, and no episode-level accumulation. See [chapter 6](06-training-pipeline.md) for the full loop.

## Tuning the reward

The only reward coefficient in the baseline is `reward_mass_income_coef`:

```rust
// crates/faf-sim/src/planner/policy/train/config.rs ~line 34 — TrainConfig (reward fields)
pub reward_mass_income_coef: f32,
```

If training is unstable or the policy behaves poorly:

- **Increase** the coefficient if mass-income changes are too small to move the gradient.
- **Decrease** the coefficient if single steps produce very large rewards that cause loss spikes.
- **Remember that the policy is not goal-seeking yet.** A policy that builds only extractors is not broken; it is correctly optimizing the only reward it was given.

When the mass-only baseline is stable, the next iteration will add a goal-completion term and milestone terms and rebalance the coefficients.

With the reward signal defined, we can look at the online training loop that turns each step into a weight update.
