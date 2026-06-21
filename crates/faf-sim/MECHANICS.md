# FAF Game Mechanics — Ground Facts for `faf-sim`

This document records the core Supreme Commander / Forged Alliance Forever (FAF)
mechanics that `faf-sim` models, plus the simplifications it intentionally
makes. It is intended as a reference for anyone extending the simulator or
writing planners/RL environments on top of it.

---

## 1. Continuous-drain build model

Unlike many RTS games where you pay the full cost upfront and then wait, FAF
uses a **continuous-drain model**:

- While a unit is being built, it drains **mass** and **energy** every second.
- The drain rate scales with the **build power** assigned to the project.
- If the available income + storage cannot cover the drain, the project
  **stalls** and effective build power drops.

### 1.1 Build power, build time, and progress

Every unit has a `BuildTime` value (in abstract work units) and every builder
has a `BuildRate` (build power). If a project is assigned build power `B`, then:

```text
progress_per_second = B / BuildTime
completion_time     = BuildTime / B
```

// crates/faf-sim/src/economy.rs ~line 82 — compute_drain

In the unit index:

- A T1 engineer typically has `BuildRate = 5`.
- An ACU typically has `BuildRate = 10`.
- A T3 land factory typically has `BuildRate = 90`.

So a Monkeylord (`BuildTime = 27500`) built by a single ACU takes:

```text
27500 / 10 = 2750 seconds ≈ 45.8 minutes
```

### 1.2 Resource drain

The mass and energy drain scale with the same progress factor:

```text
mass_per_second   = (B / BuildTime) * BuildCostMass
energy_per_second = (B / BuildTime) * BuildCostEnergy
```

Doubling the build power halves the completion time but doubles the drain per
second. The **total** resources consumed stay the same.

// crates/faf-sim/src/economy.rs ~line 99 — drain calculation inside compute_drain

---

## 2. Economy: income, storage, and stalls

### 2.1 Income

Income is produced continuously by economic units:

- The ACU produces `1 mass/sec` and `20 energy/sec` by default.
- Mass extractors, power generators, and fabricators add to this.

In `faf-sim`, the starting economy is derived by summing the production and
storage of the starting units:

// crates/faf-sim/src/sim.rs ~line 111 — derive_economy

### 2.2 Storage

Every unit with an economy section also has storage:

- ACU: `StorageMass = 650`, `StorageEnergy = 3900`.

Storage acts as a buffer. You can spend stored resources even when income is
zero, but once storage is empty the project stalls.

`faf-sim` tracks:

```rust
pub struct EconomyState {
    pub net_mass_income: f64,
    pub net_energy_income: f64,
    pub mass_storage: f64,
    pub energy_storage: f64,
    pub mass_storage_cap: f64,
    pub energy_storage_cap: f64,
}
```

// crates/faf-sim/src/economy.rs ~line 128 — EconomyState

### 2.3 Stall mechanics

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

If `stall_factor < 1.0`, the project stalled. The more constrained resource
(mass or energy) determines the stall.

// crates/faf-sim/src/economy.rs ~line 170 — apply_tick

### 2.4 Storage overflow

If storage is full, further income is wasted. `faf-sim` clamps new storage to
the cap:

```rust
new_mass_storage = (state.mass_storage + mass_income - mass_consumed)
    .min(state.mass_storage_cap)
    .max(0.0);
```

// crates/faf-sim/src/economy.rs ~line 205 — storage clamping in apply_tick

---

## 3. Project completion

A project tracks **remaining work** in `BuildTime` units:

```rust
pub struct BuildProject {
    pub target: Unit,
    pub assigned_build_power: RequestedBuildPower,
    pub remaining_work: f64,
}
```

Each tick:

```text
remaining_work -= B_effective * dt
```

When `remaining_work <= 0`, the unit is complete.

Using remaining work instead of a percentage is important because build power
can change during construction (engineers added/removed, stalls).

// crates/faf-sim/src/economy.rs ~line 223 — BuildProject

---

## 4. Builder prerequisites

Units can only be built by specific builder categories. The game encodes this
with `BUILTBY*` categories on the unit being built:

| Category | Builder required |
|---|---|
| `BUILTBYCOMMANDER` | Any ACU |
| `BUILTBYTIER1ENGINEER` | T1 engineer |
| `BUILTBYTIER2ENGINEER` | T2 engineer |
| `BUILTBYTIER3ENGINEER` | T3 engineer |
| `BUILTBYTIER1FACTORY` | T1 factory |
| `BUILTBYTIER2FACTORY` | T2 factory |
| `BUILTBYTIER3FACTORY` | T3 factory |

`faf-sim` derives this mapping from categories. Note that the current index
does not contain separate tiered-commander blueprint ids (ACU upgrades are
enhancements, not units), so `BUILTBYTIER3COMMANDER` currently maps to any ACU.

// crates/faf-sim/src/build_graph.rs ~line 20 — BuilderKind

---

## 5. Simplifications and known limitations

`faf-sim` is a research simulator, not a full game engine. It intentionally
ignores:

- **Travel time** — engineers and factories are assumed to be in place.
- **Reclaim** — no map reclaim is modeled.
- **Combat, scouting, and map control** — purely economic simulation.
- **Unit upgrades / enhancements** — ACU T2/T3 engineering upgrades are not
  modeled as separate prerequisites (yet).
- **Concurrent projects** — the simple simulator builds one thing at a time.
- **Engineer production for assist** — the simulator does not yet build extra
  engineers to speed up the target.
- **Power/mass infrastructure beyond prerequisites** — no optional mass
  extractors or power generators are built.

These limitations are acceptable for a baseline. Future phases will add
concurrency, planner heuristics, and RL environments.

---

## 6. Reference values

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
