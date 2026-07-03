# Heuristic Action Layer — Open Questions

## Agreed direction

The value/policy network will shrink to choosing one of six high-level directions:

1. `IncreaseMass`
2. `IncreaseEnergy`
3. `IncreaseBP`
4. `IncreaseEnergyStorage`
5. `Goal`
6. `UpgradeTech`

Once the network picks a direction, a heuristic layer turns it into a concrete `SimAction` (build/upgrade target + builder squad). The network no longer learns concrete edges, build power, or engineer composition.

## Open questions to answer before implementing

### 1. What does `UpgradeTech` mean concretely?

- Does it mean **factory tech** (T1 factory → T2 factory → T3 factory)?
- Does it mean **mex caps** (T2 mex → cap T2 mex, T3 mex → cap T3 mex)?
- Does it mean **both**, and the heuristic picks whichever is legal and most efficient?
- Should there be a priority order (e.g. factory tech before mex caps)?

A: upgrade only mean upgrade factory tech

### 2. Mass-efficiency formula for `IncreaseMass`

You mentioned the mex limit prevents always building T1 mexes, but we still need a scoring rule to choose between T1/T2/T3 mexes and mex caps when multiple are legal.

Options:

- **Income per mass cost** (`income_gain / mass_cost`) — favors cheap T1 mexes.
- **Payback time** (`mass_cost / income_gain`) — also favors T1 mexes.
- **Income per build-second** (`income_gain / build_time`) — favors higher-tech once build power is available.
- **Net present value over a fixed horizon** — accounts for how long the game is expected to last.
- **Hybrid**: score by income/cost, but multiply by a tech-tier bonus so T2/T3 mexes are preferred once their builders exist.

A: use payback time, we should calculate the payback time for different t1, t2, t3 and t2capped and t3capped. Then use it. Notice, mex limits refers to the total mex number in the game, so 1 t1 mex and 1 t3 mex, all considered to be the same.

### 3. `IncreaseBP` target selection

"Increase build power" is ambiguous. When this direction is chosen, the heuristic must pick one of:

- Build a T1 engineer
- Build a T2 engineer
- Build a T3 engineer
- Build a new factory
- Upgrade a factory (this overlaps with `UpgradeTech`)

What priority do you want?

One possible rule:

- If we have an idle factory and few engineers relative to income → build engineer.
- If engineer count is healthy but factory count is low → build factory.
- If `UpgradeTech` is chosen separately, then `IncreaseBP` never upgrades factories.

Is that close to what you want?

A: since we choose upgrade Tech to be one option of network output, the increase BP only refers to build more engineers. We just build most high-level engineer.

### 4. `IncreaseEnergyStorage` trigger and priority

Energy storage is unusual: it does not directly increase income, but it buffers overcharge and prevents energy overflow waste.

- Should the network even output this, or should storage be built automatically when energy storage ratio is below some threshold?
- If it is a network output, what makes it legal? (e.g. energy income above X, storage ratio below Y, no active storage project)
- Should it ever take priority over `IncreaseEnergy` when energy is stalling?

A: This is the hard part, that's why we currently decide it to be part of network output.

### 5. Build-power assignment heuristic

The power head will be removed. We need a rule for how many engineers to assign to a project.

Options:

- **Assign all capable idle builders** — fastest completion, but ties up everyone.
- **Assign enough to finish in X seconds** based on project cost and current total build power.
- **Assign a fixed fraction of total idle build power** (e.g. 50%) so other projects can run in parallel.
- **Assign by project type** — fast for cheap units, slow for expensive upgrades.

Which one matches good FAF play?

A: In game it is ually the "Assign all capable idle builders around that target". But since we could not formula that, let'do Assign all capable idle builders, but check to prevent they cause mass or energy stall.

### 6. Engineer squad selection

The squad head will be removed. The obvious heuristic is greedy by build rate: use T3 engineers first, then T2, then T1, until the target power is reached.

- Is that acceptable, or do you want to reserve high-tier engineers for high-priority projects?
- Should we ever prefer T1 engineers to conserve T3 engineers for the goal?

A: we always perfer high tech engineers

### 7. Keep MCTS or move to greedy/sampled policy?

With only six network directions and deterministic heuristics, the MCTS branching factor drops from ~25 to 6. Rollouts become mostly deterministic given a direction sequence.

- Do you want to **keep MCTS** as a cheap evaluator over the six directions?
- Do you want to **drop MCTS** and just execute the highest-probability direction (greedy) or sample from the policy?
- Do you want to keep MCTS initially and measure whether it helps before removing it?

A: keep MCTS

### 8. Fallback when the chosen direction has no legal action

If the network picks `Goal` but we do not have a T3 engineer, or picks `UpgradeTech` but no upgrade is legal, what should happen?

Options:

- Execute `Wait` for this tick.
- Fall back to a default direction (e.g. `IncreaseBP` or `IncreaseMass`).
- Penalize the network during training for choosing an illegal direction.

A: Penalize the network during training for choosing an illegal direction. **Does this mean we should also encode Tech level into network input?**

### 9. Energy stall handling

If the simulator is energy-stalled, good play usually says "build more energy" regardless of the network's current preference.

- Should the heuristic override the network and force `IncreaseEnergy` when `energy_storage < 1.0`?
- Or should the network learn this from reward shaping?

A: network should learn 1) energy-stalled is very bad, 2) use energy storage to avoid it 3) if stalled, then yes, it should learn from reward that build more energy is top task.

### 10. Mass income thresholds for upgrades

You previously mentioned tech upgrades at 40 mass/s (T2) and 80–90 mass/s (T3). Since `UpgradeTech` is now a network output, these thresholds could be:

- Hard gating rules: only allow `UpgradeTech` as a legal direction when income crosses the threshold.
- Soft biases: apply a penalty or bonus to the network's logits based on income.
- Purely learned: no thresholds, let the network decide.

Which do you prefer?

A: Purely learned: no thresholds, let the network decide.

## Follow-up questions after first review

### 11. Payback time for mass will always favor T1 mexes

If we literally pick the shortest payback time, T1 mex will almost always win until `max_mex_count` is reached. Example approximate numbers:

- T1 mex: +2 mass/s, ~36 mass → ~18 s payback
- T2 mex upgrade: +4 mass/s, ~900 mass → ~225 s payback
- T3 mex upgrade: +6 mass/s, ~2700 mass → ~450 s payback

That means the heuristic will fill every mex slot with T1 before it ever upgrades. This is valid but slower than typical good play.

**Options:**

- **A**: Keep pure payback time. Let the network compensate via `UpgradeTech` timing.
- **B**: Add a soft rule: if mass income is above a threshold and a T2/T3 engineer exists, prefer T2/T3 mex over a new T1 mex.
- **C**: Use a hybrid score that also values income density per mex slot.

Which one do you want?

A: "T1 mex will almost always win until `max_mex_count` is reached." That is how the game played, it is correct. That's why there is mex limit.
So, if the network decide to increase mass, we should consider from fill t1 mex, then upgrade to xxx. This could be done by using rules.

### 12. Concrete rule for "check to prevent mass/energy stall"

When assigning all capable idle builders to a project, we need a concrete stall-prevention rule.

**Options:**

- **A**: Hard gate — if the project would drain storage to zero or below, assign fewer engineers or emit `Wait`.
- **B**: Soft cap — assign builders until the projected one-tick drain would drop storage below a threshold (e.g. 10%).
- **C**: Affordability gate — only start the action if we can pay the full initial cost from current storage.

Which one do you want?

A: "**A**: Hard gate — if the project would drain storage to zero or below, assign fewer engineers or emit `Wait`."

### 13. How to penalize illegal directions during training

You said the network should be penalized during training for choosing an illegal direction.

**Options:**

- **A**: Action masking — set illegal direction logits to `MASK_VALUE` before softmax so the network never samples them. The reward signal still teaches it indirectly when no legal direction is available.
- **B**: Auxiliary penalty loss — add an extra loss term that punishes high raw logits for illegal directions. More complex but directly penalizes the policy's intent.

Which one do you want?

A: I think Option A is better.
