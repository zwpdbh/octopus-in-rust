# 5. Reward Shaping

A build-order episode can last thousands of simulator ticks, and the only event the ultimate user cares about is the goal finishing. If we reward the agent only at the end, training is slow and unstable. This chapter explains how `faf-sim` shapes the reward so the policy gets useful feedback at every step.

## Two reward signals

`faf-sim` uses two complementary reward functions:

1. **`compute_step_reward`** — called after every action, giving immediate feedback.
2. **`compute_terminal_bonus`** — called at the end of an episode, giving a large completion bonus or failure penalty.

The training loop combines them into a discounted return for each step.

## Per-step reward

The per-step reward is computed from the state before and after the action:

```rust
// crates/faf-sim/src/planner/mcts/train/reward.rs ~line 22 — compute_step_reward
pub(crate) fn compute_step_reward(
    prev_state: &GraphState,
    next_state: &GraphState,
    units: &Units,
) -> f32 {
    let mut reward = 0.0f32;

    // Reward increasing build power.
    let prev_bp = prev_state.total_active_build_power(units) as f32;
    let next_bp = next_state.total_active_build_power(units) as f32;
    reward += ((next_bp - prev_bp) / 20.0).clamp(-10.0, 10.0);

    // Penalise mass stall: production halts when storage is empty.
    if next_state.economy.mass_storage < 1.0 {
        reward -= 5.0;
    }

    // Penalise mass overflow: income is wasted when storage is nearly full.
    let mass_cap = next_state.economy.mass_storage_cap;
    if mass_cap > 0.0 {
        let mass_ratio = (next_state.economy.mass_storage / mass_cap) as f32;
        if mass_ratio > 0.9 {
            reward -= 5.0 * (mass_ratio - 0.9) / 0.1;
        }
    }

    // Penalise energy stall severely: it throttles build power and mass income.
    if next_state.economy.energy_storage < 1.0 {
        reward -= 20.0;
    }

    reward
}
```

The reward is intentionally simple and economy-centric:

- **Build-power growth.** When a new engineer finishes or an existing engineer upgrades, total active build power goes up. The agent is rewarded proportionally, capped at `±10` to prevent one step from dominating the return.
- **Mass stall penalty.** If mass storage drops below `1.0`, production halts, so we punish the state.
- **Mass overflow penalty.** If mass storage is above 90% of capacity, excess income is wasted. The penalty grows linearly with how full storage is.
- **Energy stall penalty.** Energy stall is more damaging than mass stall because it slows both construction and mass income, so the penalty is larger (`-20`).

## Terminal bonus

At the end of an episode, the agent receives a terminal bonus that depends on whether the goal was reached:

```rust
// crates/faf-sim/src/planner/mcts/train/reward.rs ~line 10 — compute_terminal_bonus
pub(crate) fn compute_terminal_bonus(state: &GraphState, goal_reached: bool) -> f32 {
    if goal_reached {
        1000.0 - state.time as f32 * 0.2
    } else {
        0.0
    }
}
```

- A successful episode gets a large positive bonus (`1000`) minus a small time penalty (`0.2` per second). Faster completions score higher.
- An unsuccessful episode gets no bonus. The per-step rewards may still be positive or negative, but there is no completion signal.

The terminal bonus is the dominant term in the return for successful episodes, while the per-step reward provides guidance during long episodes that have not yet reached the goal.

## Discounted returns

During training, each step's target is the discounted sum of future rewards plus the terminal bonus:

```rust
// crates/faf-sim/src/planner/mcts/train/trainer.rs ~line 650 — compute_returns
fn compute_returns(&mut self, episode: &mut Episode) {
    let step_count = episode.steps.len();
    if step_count == 0 {
        return;
    }

    let gamma = self.config.gamma;
    let mut returns = Vec::with_capacity(step_count);
    let mut g = episode.final_reward;
    for step in episode.steps.iter().rev() {
        g = step.step_reward + gamma * g;
        returns.push(g);
    }
    returns.reverse();

    let mean = returns.iter().sum::<f32>() / step_count as f32;
    let std = (returns.iter().map(|r| (r - mean).powi(2)).sum::<f32>() / step_count as f32)
        .sqrt()
        .max(1e-6);

    for (step, ret) in episode.steps.iter_mut().zip(returns) {
        step.return_value = (ret - mean) / std;
    }
}
```

The returns are standardized (mean-zero, unit-variance) across the episode. This reduces variance in the REINFORCE gradient estimate and makes the optimizer less sensitive to the absolute scale of rewards.

## Why not count-based rewards?

Earlier versions of `faf-sim` rewarded the agent for owning specific units or reaching tech milestones. These rewards had two problems:

1. They encoded human assumptions about what a good plan looks like, which can be wrong or faction-specific.
2. They could be gamed by building units that do not actually help reach the goal faster.

The current reward signal is based on throughput and resource health instead. The agent is free to discover unusual build orders as long as they finish the goal quickly without wasting resources.

## Tuning the reward

The reward coefficients are hyperparameters. If training is unstable, consider:

- Scaling the build-power reward to match the typical size of build-power changes in your simulator configuration.
- Adjusting the energy stall penalty if the policy is too cautious about energy.
- Adjusting the mass overflow penalty if the policy hoards mass.
- Changing the terminal bonus magnitude relative to the per-step rewards.

A good rule of thumb is that the terminal bonus should dominate the return for a successful episode, while the per-step rewards should keep episodes that have not reached the goal from collapsing to zero gradient.

With the reward signal defined, we can look at the training loop that turns episodes into weight updates.
