# 01 — Ground Facts: The Continuous-Drain Build Model

Before we can optimize a build order, we need to agree on how building actually
works in FAF. This document records the core mechanics `faf-sim` models and the
simplifications it intentionally makes.

---

## 1. Build power, build time, and progress

Every unit has a `BuildTime` value (in abstract work units) and every builder has
a `BuildRate` (build power). If a project is assigned build power `B`, then:

```text
progress_per_second = B / BuildTime
completion_time     = BuildTime / B
```

In the unit index:

- A T1 engineer typically has `BuildRate = 5`.
- An ACU typically has `BuildRate = 10`.
- A T3 land factory typically has `BuildRate = 90`.

So a Monkeylord (`BuildTime = 27500`) built by a single ACU takes:

```text
27500 / 10 = 2750 seconds ≈ 45.8 minutes
```

The function that turns build power into drain rates is:

```rust
// crates/faf-sim/src/economy.rs ~line 82 — compute_drain
pub fn compute_drain(unit: &Unit, assigned_build_power: RequestedBuildPower) -> Option<BuildDrain> {
    let economy = unit.economy.as_ref()?;
    let build_time = economy.build_time?;
    let total_mass = economy.build_cost_mass?;
    let total_energy = economy.build_cost_energy?;

    if build_time <= 0.0 {
        return None;
    }

    let power = assigned_build_power.0;
    let progress_per_second = power / build_time;
    let completion_time_seconds = 1.0 / progress_per_second;

    let mass_per_second = progress_per_second * total_mass;
    let energy_per_second = progress_per_second * total_energy;

    Some(BuildDrain {
        mass_per_second,
        energy_per_second,
        progress_per_second,
        total_mass,
        total_energy,
        completion_time_seconds,
        assigned_build_power,
    })
}
```

## 2. Resource drain

The mass and energy drain scale with the same progress factor:

```text
mass_per_second   = (B / BuildTime) * BuildCostMass
energy_per_second = (B / BuildTime) * BuildCostEnergy
```

Doubling the build power halves the completion time but doubles the drain per
second. The **total** resources consumed stay the same.

```rust
// crates/faf-sim/src/economy.rs ~line 104 — drain calculation inside compute_drain
let mass_per_second = progress_per_second * total_mass;
let energy_per_second = progress_per_second * total_energy;
```

## 3. Why this matters for optimization

Because drain scales with build power, "more engineers" is not automatically
better. Adding build power only helps if the economy can feed it. The optimizer's
job is to balance three things:

1. **Prerequisites** — you cannot build a T3 engineer without a T3 factory.
2. **Build power** — more BP finishes projects faster, but only if resourced.
3. **Economy** — income and storage must cover the drain or the project stalls.

## 4. Reference values

From the embedded unit index (`plugins/faf-units/data/faf_units.json`):

| Unit | BuildRate | BuildTime | BuildCostMass | BuildCostEnergy |
|---|---|---|---|---|
| Cybran ACU (URL0001) | 10 | 6,000,000 | 2,000 | 5,000,000 |
| Cybran T1 eng (URL0105) | 5 | 260 | 52 | 260 |
| Cybran T1 factory (URB0101) | 20 | 300 | 240 | 2,100 |
| Cybran T2 factory (URB0201) | 40 | 2,300 | 1,410 | 11,200 |
| Cybran T3 factory (URB0301) | 90 | 12,000 | 5,220 | 47,400 |
| Cybran T3 eng (URL0309) | 32.5 | 1,560 | 312 | 1,560 |
| Monkeylord (URL0402) | — | 27,500 | 20,000 | 260,000 |

ACU default economy:

- `ProductionPerSecondMass = 1.0`
- `ProductionPerSecondEnergy = 20.0`
- `StorageMass = 650.0`
- `StorageEnergy = 3900.0`

## 5. Study questions

1. Why does the Monkeylord take ~45 minutes with one ACU but not 27,500 / 5 = 5,500
seconds with one T1 engineer? (Hint: who can build it?)
2. Compute the mass and energy drain per second for a Monkeylord assisted by one
T3 engineer (`BuildRate = 32.5`).
3. If you assign a second T3 engineer to the same Monkeylord, what happens to the
drain and the completion time? What resource is most likely to stall first?

## 6. Experiment

Run the CLI to inspect a target's raw economy:

```bash
# The deps command does not show economy numbers directly, but it confirms who
# can build the target. Use the source or write a small Rust test to print drain.
cargo run --bin faf-sim -- deps -c monkeylord
```

Next: [02-prerequisites-and-tech-chains.md](./02-prerequisites-and-tech-chains.md)
