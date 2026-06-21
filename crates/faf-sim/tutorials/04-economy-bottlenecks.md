# 04 — Economy Bottlenecks: Income, Storage, and Stalls

A project can only progress as fast as the economy allows. This document explains
how `faf-sim` models income, storage, stalls, and overflow.

---

## 1. Economy state

The simulator tracks a small state vector:

```rust
// crates/faf-sim/src/economy.rs ~line 128 — EconomyState
pub struct EconomyState {
    pub net_mass_income: f64,
    pub net_energy_income: f64,
    pub mass_storage: f64,
    pub energy_storage: f64,
    pub mass_storage_cap: f64,
    pub energy_storage_cap: f64,
}
```

Income is produced continuously by economic units. The starting economy is
derived by summing the production and storage of the starting units:

```rust
// crates/faf-sim/src/sim.rs ~line 111 — derive_economy
pub fn derive_economy(units: &[&Unit]) -> EconomyState {
    let mut net_mass_income = 0.0;
    let mut net_energy_income = 0.0;
    let mut mass_storage = 0.0;
    let mut energy_storage = 0.0;

    for unit in units {
        if let Some(econ) = &unit.economy {
            net_mass_income += econ.production_per_second_mass.unwrap_or(0.0);
            net_energy_income += econ.production_per_second_energy.unwrap_or(0.0);
            mass_storage += econ.storage_mass.unwrap_or(0.0);
            energy_storage += econ.storage_energy.unwrap_or(0.0);
        }
    }

    EconomyState {
        net_mass_income,
        net_energy_income,
        mass_storage,
        energy_storage,
        mass_storage_cap: mass_storage,
        energy_storage_cap: energy_storage,
    }
}
```

## 2. Stall mechanics

During a tick, the simulator computes the maximum sustainable fraction of the
requested drain before either resource would hit zero:

```text
mass_factor   = (mass_storage + mass_income * dt) / (requested_mass * dt)
energy_factor = (energy_storage + energy_income * dt) / (requested_energy * dt)
stall_factor  = min(mass_factor, energy_factor, 1.0)
```

Effective build power becomes:

```text
B_effective = B_requested * stall_factor
```

The implementation is in `apply_tick`:

```rust
// crates/faf-sim/src/economy.rs ~line 170 — apply_tick
pub fn apply_tick(requested: &BuildDrain, state: &EconomyState, dt: f64) -> TickResult {
    let mass_income = state.net_mass_income * dt;
    let energy_income = state.net_energy_income * dt;

    let requested_mass = requested.mass_per_second * dt;
    let requested_energy = requested.energy_per_second * dt;

    let mass_factor = if requested_mass <= 0.0 {
        1.0
    } else {
        let available = (state.mass_storage + mass_income).max(0.0);
        (available / requested_mass).min(1.0)
    };

    let energy_factor = if requested_energy <= 0.0 {
        1.0
    } else {
        let available = (state.energy_storage + energy_income).max(0.0);
        (available / requested_energy).min(1.0)
    };

    let effective_factor = mass_factor.min(energy_factor);
    let effective_build_power = requested
        .assigned_build_power
        .to_effective(effective_factor);

    let mass_consumed = requested_mass * effective_factor;
    let energy_consumed = requested_energy * effective_factor;

    let new_mass_storage = (state.mass_storage + mass_income - mass_consumed)
        .min(state.mass_storage_cap)
        .max(0.0);
    let new_energy_storage = (state.energy_storage + energy_income - energy_consumed)
        .min(state.energy_storage_cap)
        .max(0.0);

    TickResult {
        effective_build_power,
        mass_consumed,
        energy_consumed,
        new_mass_storage,
        new_energy_storage,
        energy_stalled: effective_factor < 1.0 && energy_factor <= mass_factor,
        mass_stalled: effective_factor < 1.0 && mass_factor <= energy_factor,
    }
}
```

## 3. Storage overflow

If storage is full, further income is wasted. `faf-sim` clamps new storage to the
cap:

```rust
// crates/faf-sim/src/economy.rs ~line 205 — storage clamping in apply_tick
let new_mass_storage = (state.mass_storage + mass_income - mass_consumed)
    .min(state.mass_storage_cap)
    .max(0.0);
```

This matters for optimization: a build order that leaves storage full while
building cheap units is wasting income that could have been spent on the goal.

## 4. What determines the bottleneck?

For most expensive units, energy is the binding resource early on. The ACU starts
with only `20 energy/sec`, while a Monkeylord assisted by a single ACU drains:

```text
energy_per_second = (10 / 27500) * 260000 ≈ 94.5
```

So the ACU alone cannot even sustain its own assist on a Monkeylord. Storage
covers the gap for a while, then the build stalls until more energy income is
available.

## 5. Study questions

1. Why does the ACU's starting energy storage matter for the first few seconds
of building a Monkeylord?
2. If mass storage is full and energy storage is empty, which resource is the
bottleneck?
3. How would the stall factor change if `dt` were smaller?

## 6. Experiment

Simulate the default chain for a Monkeylord and observe the timeline. Which unit
finishes just before storage would run out?

```bash
cargo run --bin faf-sim -- simulate -c monkeylord
```

Next: [05-build-power-investment.md](./05-build-power-investment.md)
