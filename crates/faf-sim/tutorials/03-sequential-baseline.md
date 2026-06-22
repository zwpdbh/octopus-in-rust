# 03 — Observing the Economy: From Units to Net Flow

The simulator no longer uses a hard-coded sequential baseline. Instead, it
starts from the same observation a player has: a collection of owned units.
From those units it derives the current economy state — income, storage, and
net flow.

---

## 1. Owned units are economic actors

Every unit can play one or more economic roles:

- **Producer** — adds mass or energy each second (ACU, mex, pgen).
- **Consumer** — removes resources through maintenance (mex, radar, shields).
- **Builder** — provides build power to spend resources faster (ACU, engineers,
  factories).
- **Storage** — contributes mass/energy capacity (ACU, storage buildings).

These roles are modeled as traits:

```rust
// crates/faf-sim/src/economy.rs ~line 191 — EcoProducer / EcoConsumer
trait EcoProducer {
    fn production(&self) -> EcoFlow;
}

trait EcoConsumer {
    fn consumption(&self) -> EcoFlow;
}
```

## 2. `EcoFlow`: the atomic observation

```rust
// crates/faf-sim/src/economy.rs ~line 143 — EcoFlow
pub struct EcoFlow {
    pub mass_per_second: f64,
    pub energy_per_second: f64,
}
```

Positive values mean resources enter the economy; negative values mean they
leave it. The in-game economy overlay is exactly this: production minus
consumption.

## 3. Deriving the economy state

`derive_economy` sums storage from all owned units and computes net flow from
production minus maintenance consumption:

```rust
// crates/faf-sim/src/sim.rs ~line 26 — derive_economy
pub fn derive_economy(units: &[&Unit]) -> EconomyState {
    let mut mass_storage = 0.0;
    let mut energy_storage = 0.0;

    for unit in units {
        if let Some(econ) = &unit.economy {
            mass_storage += econ.storage_mass.unwrap_or(0.0);
            energy_storage += econ.storage_energy.unwrap_or(0.0);
        }
    }

    let net = summarize_economy(units, &[]);

    EconomyState {
        net_mass_income: net.mass_per_second,
        net_energy_income: net.energy_per_second,
        mass_storage,
        energy_storage,
        mass_storage_cap: mass_storage,
        energy_storage_cap: energy_storage,
    }
}
```

`summarize_economy` is the key merge step:

```rust
// crates/faf-sim/src/economy.rs ~line 232 — summarize_economy
pub fn summarize_economy(owned_units: &[&Unit], active_projects: &[&BuildProject]) -> EcoFlow {
    let production: EcoFlow = owned_units.iter().map(|u| u.production()).sum();
    let maintenance: EcoFlow = owned_units.iter().map(|u| u.consumption()).sum();
    let construction: EcoFlow = active_projects.iter().map(|p| p.consumption()).sum();
    production - maintenance - construction
}
```

## 4. Example: ACU alone vs. ACU + T1 mex

```bash
$ cargo test -p faf-sim derive_economy_subtracts_maintenance -- --nocapture
```

The test verifies:

- ACU alone: `+1 mass/s`, `+20 energy/s`.
- ACU + T1 mex: `+3 mass/s`, `+18 energy/s` (the mex adds `+2 mass/s` but costs
  `-2 energy/s` maintenance).

This is why a player can look at the economy overlay and see negative energy
income: maintenance is real.

## 5. Study questions

1. Why is it important that `derive_economy` subtracts maintenance consumption?
2. If you own 8 T1 mexes and one ACU, what is your net mass income? Net energy
   income?
3. Where does construction consumption appear in `derive_economy`? Why is it
   passed as an empty slice there?

## 6. Experiment

Inspect the economy state for different starting unit mixes:

```rust
// crates/faf-sim/src/sim.rs ~line 42 — derive_economy_subtracts_maintenance
let state = derive_economy(&[acu, t1_mex, t1_mex]);
println!("mass {} energy {}", state.net_mass_income, state.net_energy_income);
```

Next: [04-economy-bottlenecks.md](./04-economy-bottlenecks.md)
