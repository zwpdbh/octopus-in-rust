# 5. Reward Shaping

A build-order episode can last thousands of simulator ticks, and the only event the ultimate user cares about is the goal finishing. If we reward the agent only at the end, training is slow and unstable. This chapter explains how `faf-sim` shapes the reward so the policy gets useful feedback at every step.

## Three reward signals

`faf-sim` uses three complementary reward functions:

1. **`compute_step_reward`** — called after every action, giving immediate feedback on economy and build-power growth.
2. **`MilestoneTracker`** — gives one-time bonuses for reaching key tech levels.
3. **`compute_terminal_bonus`** — called at the end of an episode, giving a large completion bonus or failure penalty.

The training loop combines them into a discounted return for each step.

## Per-step reward

The per-step reward is computed from the state before and after the action:

```rust
// crates/faf-sim/src/planner/mcts/train/reward.rs ~line 22 — compute_step_reward
pub(crate) fn compute_step_reward(
    prev_state: &SimulationState,
    next_state: &SimulationState,
    units: &Units,
) -> f32 {
    let mut reward = 0.0f32;

    // Reward increasing build power.
    let prev_bp = prev_state.total_active_build_power(units) as f32;
    let next_bp = next_state.total_active_build_power(units) as f32;
    reward += ((next_bp - prev_bp) / 20.0).clamp(-10.0, 10.0);

    // Reward increasing mass income (drives the eco -> BP -> spend loop).
    let prev_mass = prev_state.economy.net_mass_income as f32;
    let next_mass = next_state.economy.net_mass_income as f32;
    let mass_delta = (next_mass - prev_mass).clamp(-30.0, 30.0);
    reward += (mass_delta / 10.0).clamp(-10.0, 10.0);

    // Reward increasing power income (supports more BP).
    let prev_energy = prev_state.economy.net_energy_income as f32;
    let next_energy = next_state.economy.net_energy_income as f32;
    let energy_delta = (next_energy - prev_energy).clamp(-100.0, 100.0);
    reward += (energy_delta / 50.0).clamp(-5.0, 5.0);

    // Penalise high mass storage and overflow.
    let mass_cap = next_state.economy.mass_storage_cap;
    if mass_cap > 0.0 {
        let mass_ratio = (next_state.economy.mass_storage / mass_cap) as f32;
        if mass_ratio > 0.7 {
            reward -= 3.0 * (mass_ratio - 0.7) / 0.3;
        }
        if mass_ratio > 0.9 {
            reward -= 5.0 * (mass_ratio - 0.9) / 0.1;
        }
    }

    // Penalise energy stall severely: it throttles build power and mass income.
    if next_state.economy.energy_storage < 1.0 {
        reward -= 20.0;
    }

    // Small penalty for mass stall.
    if next_state.economy.mass_storage < 1.0 {
        reward -= 1.0;
    }

    reward
}
```

The per-step reward is economy-centric and designed to keep the expansion chain moving:

- **Build-power growth.** When a new engineer finishes or an existing engineer upgrades, total active build power goes up. The agent is rewarded proportionally, capped at `±10`.
- **Mass income growth.** Growing mass income is necessary, but it must be paired with BP growth to avoid overflow. Capped at `±10`.
- **Power income growth.** More BP consumes more energy, so the agent must grow power to keep the chain running. Capped at `±5`.
- **Mass storage pressure.** Storage above 70% is penalized, with a stronger penalty above 90%. This encourages the agent to spend mass quickly instead of hoarding it.
- **Energy stall penalty.** Energy stall is more damaging than mass stall because it slows both construction and mass income, so the penalty is larger (`-20`).
- **Mass stall penalty.** Mass stall is annoying but not catastrophic; it receives a small penalty (`-1`).

## Terminal bonus

At the end of an episode, the agent receives a terminal bonus that depends on whether the goal was reached:

```rust
// crates/faf-sim/src/planner/mcts/train/reward.rs ~line 10 — compute_terminal_bonus
pub(crate) fn compute_terminal_bonus(state: &SimulationState, goal_reached: bool) -> f32 {
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

## Tech milestones

For expensive T4 targets the terminal bonus is too sparse on its own: the agent must learn a long prerequisite chain before it ever sees a positive completion signal. `MilestoneTracker` adds one-time bonuses for unlocking key technologies:

```rust
// crates/faf-sim/src/planner/mcts/train/reward.rs ~line 77 — MilestoneTracker
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct MilestoneTracker {
    t2_factory: bool,
    t3_factory: bool,
    t3_engineer: bool,
}

impl MilestoneTracker {
    pub(crate) fn update(&mut self, state: &SimulationState, _units: &Units) -> f32 {
        let mut bonus = 0.0f32;

        if !self.t2_factory && state.has_completed_unit(&UnitKind::Factory(TechLevel::T2)) {
            self.t2_factory = true;
            bonus += 50.0;
        }
        if !self.t3_factory && state.has_completed_unit(&UnitKind::Factory(TechLevel::T3)) {
            self.t3_factory = true;
            bonus += 150.0;
        }
        if !self.t3_engineer && state.has_completed_unit(&UnitKind::Engineer(TechLevel::T3)) {
            self.t3_engineer = true;
            bonus += 300.0;
        }

        bonus
    }
}
```

The bonuses are given **once per episode**, so the agent cannot farm them by repeatedly rebuilding the same unit. They are also much smaller than the terminal bonus (`1000`), so finishing the goal remains the strongest signal.

## Discounted returns

During training, each step's target is the discounted sum of future rewards plus the terminal bonus:

```rust
// crates/faf-sim/src/planner/mcts/train/trainer/update.rs ~line 15 — compute_returns
pub(crate) fn compute_returns(&mut self, episode: &mut Episode) {
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

## Why these milestones?

Earlier versions of `faf-sim` either had no milestone rewards or rewarded generic unit ownership. Generic ownership rewards can be gamed: the agent might build a unit, collect the reward, and then waste resources on something unrelated.

The current milestones are designed to avoid that:

1. **They are one-time per episode.** You cannot farm them by rebuilding the same unit.
2. **They are genuine prerequisites.** A T4 experimental requires a T3 engineer to start it, so reaching T3 engineer is a meaningful sub-goal regardless of the exact build path.
3. **They are smaller than the terminal bonus.** The agent still learns that finishing the goal is better than merely unlocking tech.

The per-step rewards encourage a healthy economy and BP growth, while the milestones encourage the specific tech progression needed for end-game units.

## Tuning the reward

The reward coefficients are hyperparameters. If training is unstable, consider:

- Scaling the build-power and mass-income rewards so they are roughly balanced.
- Adjusting the energy stall penalty if the policy is too cautious about energy.
- Adjusting the mass storage pressure if the policy hoards mass or ignores overflow.
- Changing the milestone bonuses if the policy techs too slowly or too aggressively.
- Changing the terminal bonus magnitude relative to the per-step rewards and milestones.

A good rule of thumb is that the terminal bonus should dominate the return for a successful episode, while the per-step rewards and milestones should keep episodes that have not reached the goal from collapsing to zero gradient.

With the reward signal defined, we can look at the training loop that turns episodes into weight updates.
