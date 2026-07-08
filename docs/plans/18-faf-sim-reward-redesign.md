# Plan: Redesign `faf-sim` Training Reward with Goal-Readiness Rollouts

## Goal

Replace the current per-step mass-income-delta reward with a rollout-based reward that teaches the policy two things:

1. **Eco expansion:** which normal direction grows the economy and spending power most effectively over the next 60 seconds.
2. **Goal timing:** when the economy is strong enough to start the final goal, using a 5-minute finish-time test.

The existing `EdgeCategory::Goal` direction is reused as the "start final goal" decision. No new network output dimension is needed.

## Key insight

`EdgeCategory::ALL` already contains a `Goal` variant:

```rust
// crates/faf-sim/src/planner/plan_graph.rs ~line 62
Goal,
```

`direction_to_action` already maps it to `SimAction::BuildGoal`:

```rust
// crates/faf-sim/src/planner/policy/heuristic.rs ~line 38
EdgeCategory::Goal => pick_goal_action(state, units, config, goal),
```

So the architectural change is not "add a new direction." It is:

- Give `EdgeCategory::Goal` a meaningful, non-myopic reward.
- Ensure the policy explores `Goal` often enough to learn the right eco threshold.

## Files to modify

| File | What changes |
|---|---|
| `crates/faf-sim/src/planner/policy/train/reward.rs` | Complete rewrite. New reward computation with rollout helpers. |
| `crates/faf-sim/src/planner/policy/train/rollout.rs` *(new)* | Rollout engine: eco rollout, rush rollout, energy-stall tracking. |
| `crates/faf-sim/src/planner/policy/train/trainer/run_episode.rs` | Call new reward function; add epsilon-greedy exploration for `Goal`. |
| `crates/faf-sim/src/planner/policy/direction_planner.rs` | Inference-time `Goal` handling stays similar; may reuse rollout helpers for deterministic mode. |
| `crates/faf-sim/src/planner/policy/heuristic.rs` | Power-storage delay; factory-upgrade cost/time tweak. |
| `crates/faf-sim/src/simulation/state.rs` or economy module | Expose `mass_drain`, `energy_income`, `energy_drain`, `mass_stored`, `energy_stored` if not already public. |

## Architecture change: add a separate rush head

The network will now have two heads on a shared backbone:

1. **Eco head:** 5 outputs for `IncreaseMass`, `IncreaseEnergy`, `IncreaseBP`, `IncreaseEnergyStorage`, `UpgradeTech`.
2. **Rush head:** 1 output `p_rush` — probability that the economy is ready to start the final goal.

```text
state_features
      ↓
  backbone
      ↓
   latent
   ↙     ↘
eco_head  rush_head
[5]        [1]
```

This decouples "which eco direction is best?" from "should I rush now?" The rush head is trained directly on the 5-minute rush rollout outcome, which is a cleaner signal than competing in a shared softmax.

## Step-by-step implementation

### Step 1: Add rollout primitives

Create `crates/faf-sim/src/planner/policy/train/rollout.rs` with the following helpers.

#### `struct RolloutResult`

```rust
pub(crate) struct RolloutResult {
    /// Total mass actually spent during the rollout.
    pub mass_spent: f32,
    /// Longest contiguous energy-stall duration in seconds.
    pub longest_energy_stall_secs: f32,
    /// Whether the final goal completed during the rollout.
    pub goal_finished: bool,
    /// Time in seconds until goal completion, if it finished.
    pub time_to_finish_secs: Option<f32>,
    /// Whether stored mass exceeded 50% of capacity at the end of the rollout.
    /// Intermediate spikes are ignored because the mass may be spent later.
    pub mass_hoarded: bool,
}
```

#### `fn eco_rollout`

Simulate `horizon_secs` from a given state, assigning 80% of total engineer build power to a **phantom final-goal project**.

- Create a phantom project with the same mass/energy drain profile as the real final goal. It consumes resources like a real project but its progress is discarded after the rollout; it cannot actually complete the goal and does not mutate the real plan graph.
- The phantom project may be unbuildable in reality (e.g., prerequisites missing). That is fine — the rollout only measures how much mass the economy can effectively spend when asked to fund the goal.
- At each `dt`, assign 80% of the state's total engineer build power to this phantom goal project.
- Tick the state forward by `dt`.
- Accumulate `mass_spent` on the phantom project.
- Track energy stall duration.
- At the end of the rollout, check whether stored mass still exceeds 50% of capacity.

The purpose of this rollout is to measure the economy's ability to fund the final goal, not to simulate actual eco expansion.

#### `fn rush_rollout`

Simulate from a state where the final goal has just been started for real.

- This is the real `BuildGoal` project, not a phantom. Progress counts toward actual completion.
- Assign available engineers (prefer T3) to the goal project.
- Do not start new engineer/factory projects.
- Allow minimal eco maintenance only if the goal would otherwise stall.
- Run until the goal finishes or a 5-minute cap is hit.
- Return `goal_finished`, `time_to_finish_secs`, and `longest_energy_stall_secs`.

#### `fn is_energy_stall(state)`

Returns true when `energy_stored == 0` and `energy_drain > energy_income`.

### Step 2: Rewrite `compute_step_reward`

Current signature:

```rust
// crates/faf-sim/src/planner/policy/train/reward.rs ~line 14
pub(crate) fn compute_step_reward(
    prev_state: &SimulationState,
    next_state: &SimulationState,
    _units: &Units,
    config: &TrainConfig,
) -> f32
```

New signature (conceptual):

```rust
pub(crate) fn compute_step_reward(
    prev_state: &SimulationState,
    next_state: &SimulationState,
    chosen_direction: EdgeCategory,
    action: &SimAction,
    units: &Units,
    planner_config: &PlannerConfig,
    goal: &Goal,
) -> f32
```

#### Branch 1: normal eco direction

If `chosen_direction != EdgeCategory::Goal`:

1. `prev = eco_rollout(prev_state, 60.0)` — spend 80% BP on a placeholder goal project.
2. `next = eco_rollout(next_state, 60.0)` — same.
3. `delta = next.mass_spent - prev.mass_spent`
4. `reward = delta * mass_reward_coef`
5. If `delta <= 0`: add small negative penalty (`wasted_action_penalty`).
6. If `next.mass_hoarded`: add negative penalty (`hoarding_penalty`).
7. If `next.longest_energy_stall_secs > 5.0`: add large negative penalty (`stall_penalty`).

This rewards actions that make the economy better at funding the final goal.

#### Branch 2: `EdgeCategory::Goal`

If `chosen_direction == EdgeCategory::Goal`:

1. `rush = rush_rollout(next_state, goal, 300.0)`
2. If `rush.goal_finished`:
   - `reward = goal_finish_base_reward + goal_time_reward_coef * (300.0 - rush.time_to_finish_secs.unwrap())`
3. Else:
   - `reward = goal_too_early_penalty`
4. If `rush.longest_energy_stall_secs > 5.0`:
   - `reward += stall_penalty`

### Step 3: Decision logic with two heads

In `run_episode.rs` and `direction_planner.rs`, the decision becomes:

```text
eco_logits = network.eco_head(state_features)
rush_p     = network.rush_head(state_features)   # sigmoid output in [0, 1]

best_eco_direction = argmax over legal eco directions

if EdgeCategory::Goal is legal:
    # Explore: sometimes ignore the rush head and try Goal anyway
    with probability epsilon:
        chosen_direction = Goal
    else if rush_p > rush_threshold:
        chosen_direction = Goal
    else:
        chosen_direction = best_eco_direction
else:
    chosen_direction = best_eco_direction
```

`epsilon` starts high (e.g., 0.3) and decays. `rush_threshold` can be 0.5, tuned later.

This keeps eco expansion deterministic while exploring `Goal` across many eco levels.

### Step 4: Action execution tweaks in `heuristic.rs`

#### Power storage delay

In `pick_storage_action`:

1. Return `SimAction::Wait` and set a delayed-build marker, or
2. More simply: assign no builders immediately, but record that the storage project should receive builders after 10 seconds of energy accumulation.

Because `SimAction::Wait` advances time, the simplest implementation is:

- If the storage project was just started and less than 10s of energy has accumulated, return `SimAction::Wait` instead of assigning builders.
- This requires tracking project start time or inferring it from state.

If state does not expose project start time, add a field to `SimulationState` or to the project node.

#### Factory upgrade cost tweak

In `pick_upgrade_action`:

1. Select the target factory upgrade as before.
2. Find 3 additional same-tier engineers (not currently assigned).
3. Add their mass and energy cost to the upgrade target's cost, and add their build power to the project's effective build rate.
4. Do not actually change the target's nominal build time; instead, pretend the combined squad builds it.

Implementation options:

- Mutate the project inside `SimulationState` after starting it.
- Or return a modified `SimAction::Upgrade` with extra builders and adjust the project's effective rate in the simulator.

The simplest path: after `execute_action` returns, post-process the state to add 3 helper engineers to the upgrade project.

### Step 5: Train both heads

`update.rs` now performs two updates per step:

#### Eco head update

Use REINFORCE with the eco rollout reward for the chosen eco direction:

```text
loss_eco = -log π_eco(chosen_eco_direction) * reward_eco
```

If `chosen_direction == Goal`, the eco head is not updated for this step.

#### Rush head update

Use a binary cross-entropy or MSE loss against the rush rollout outcome:

```text
if chosen_direction == Goal:
    target_rush = 1.0 if goal_finished_within_5min else 0.0
    loss_rush = (rush_p - target_rush)²
else:
    # Optional: teach the head that non-Goal steps mean "do not rush"
    target_rush = 0.0
    loss_rush = (rush_p - target_rush)²
```

```text
loss_total = loss_eco + loss_rush * rush_loss_weight
```

### Step 6: Inference-time use of rollouts (optional)

`direction_planner.rs` currently picks directions using network logits. After training, this is fine. Optionally, during inference with `deterministic = true`, you can also run the rollout scorer and pick the direction with the best rollout score. This makes the trained network a proposal generator and the simulator the final arbiter.

For the first version, keep inference as network-only to avoid doubling decision-time cost.

### Step 7: Expose simulator economy fields

Verify that the following are readable from `SimulationState` or its economy sub-struct:

- `mass_income`
- `mass_drain` / `mass_spent`
- `mass_stored` and `mass_storage_capacity`
- `energy_income`
- `energy_drain`
- `energy_stored`
- whether mass overflowed

Add getters if missing.

### Step 8: Hyperparameters

Add to `TrainConfig`:

```rust
pub eco_rollout_horizon_secs: f32,       // 60.0
pub rush_rollout_cap_secs: f32,          // 300.0
pub energy_stall_threshold_secs: f32,    // 5.0
pub mass_storage_hoarding_ratio: f32,    // 0.5
pub mass_reward_coef: f32,
pub wasted_action_penalty: f32,
pub hoarding_penalty: f32,
pub stall_penalty: f32,
pub goal_finish_base_reward: f32,
pub goal_time_reward_coef: f32,
pub goal_too_early_penalty: f32,
pub epsilon_start: f32,                  // 0.3
pub epsilon_end: f32,                    // 0.01
pub epsilon_decay_episodes: usize,       // e.g., 1000
pub rush_threshold: f32,                 // 0.5
pub rush_loss_weight: f32,               // 1.0
```

## Testing plan

1. **Unit tests for rollout helpers**
   - Eco rollout on a state with strong economy should spend more mass on the placeholder goal than a weak economy.
   - Rush rollout on a state too weak to finish the goal should return `goal_finished = false`.
   - Rush rollout on a state strong enough should return `goal_finished = true` within 5 minutes.
   - Energy stall detection correctly identifies zero-storage + drain > income.

2. **Network architecture tests**
   - `eco_head` outputs 5 values.
   - `rush_head` outputs 1 value in `[0, 1]` after sigmoid.
   - Both heads share the backbone.

3. **Reward sanity checks**
   - A direction that increases mass spending should get positive eco reward.
   - `Goal` picked with weak eco should get negative rush target.
   - `Goal` picked with strong eco should get positive rush target.

4. **Training smoke test**
   - Run a short training run and verify that `rush_p` increases as mass income grows.
   - Log average reward per episode, direction distribution, and rush_head probability over time.

5. **Ablation test**
   - Train one run with the new reward and rush head.
   - Train one run with the old mass-delta reward.
   - Compare final policy behavior on a fixed set of initial states.

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| Rollouts are too slow | Cap rush at 5 min; cap eco at 60 s; profile before full training. |
| Policy never picks `Goal` | Epsilon-greedy with high initial epsilon; log rush_head probability and direction distribution. |
| Reward shaping overwhelms learning | Start with small coefficients; tune via ablation. |
| Factory-upgrade tweak breaks build-time invariants | Add unit tests; keep the tweak isolated in `heuristic.rs`. |
| Power-storage delay causes state inconsistency | Track delay per project; use `Wait` fallback. |

## Open decisions before implementation

1. Should `rush_rollout` maintain any eco growth during the rush, or purely focus on the goal?
2. Should inference use rollouts, or only training?
3. Should the rush head also be updated on non-Goal steps with target 0.0?

## Summary of the core loop after changes

```text
for each training step:
    eco_logits = network.eco_head(state_features)
    rush_p     = network.rush_head(state_features)

    best_eco = argmax over legal eco directions

    if Goal is legal and (explore or rush_p > threshold):
        chosen_direction = Goal
    else:
        chosen_direction = best_eco

    action = direction_to_action(chosen_direction)
    prev_state = state.clone()
    execute_action(state, action)

    if chosen_direction == Goal:
        result = rush_rollout(state, 300s)
        reward_eco = 0
        target_rush = 1.0 if result.goal_finished else 0.0
    else:
        prev = eco_rollout(prev_state, 60s)
        next = eco_rollout(state, 60s)
        reward_eco = eco_improvement_reward(prev, next)
        target_rush = 0.0

    update eco_head with REINFORCE loss using reward_eco
    update rush_head with (rush_p - target_rush)²
```

This keeps eco expansion deterministic, isolates the rush decision, and trains the rush head directly on the outcome of the 5-minute rollout.
