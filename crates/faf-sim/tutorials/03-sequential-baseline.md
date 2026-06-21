# 03 — Sequential Baseline with `SimpleSimulator`

The simplest way to simulate a build order is to build one unit after another,
assigning all available build power to the current project. This gives us a
baseline completion time we can compare against later, smarter planners.

---

## 1. What `SimpleSimulator` does

`SimpleSimulator` owns the starting units, derives the economy from them, and
steps through a sequence of targets. Each completed unit becomes available for
subsequent projects.

```rust
// crates/faf-sim/src/sim.rs ~line 23 — SimpleSimulator
#[derive(Debug, Clone)]
pub struct SimpleSimulator<'a> {
    pub owned_units: Vec<&'a Unit>,
    pub state: EconomyState,
    pub current_time: f64,
    pub dt: f64,
}
```

The entry point is `simulate_sequence`:

```rust
// crates/faf-sim/src/sim.rs ~line 55 — simulate_sequence
pub fn simulate_sequence(&mut self, sequence: &[&'a Unit]) -> Vec<BuildEvent> {
    let mut events = Vec::with_capacity(sequence.len());
    for target in sequence {
        let event = self.build_unit(target);
        self.owned_units.push(target);
        self.state = derive_economy(&self.owned_units);
        events.push(event);
    }
    events
}
```

## 2. Building a single unit

Each unit is built tick-by-tick. The assigned build power is the sum of all
owned builders, and the economy state is updated every tick.

```rust
// crates/faf-sim/src/sim.rs ~line 68 — build_unit
fn build_unit(&mut self, target: &'a Unit) -> BuildEvent {
    let mut project = BuildProject::new(target).expect("unit must have economy data");
    project.assigned_build_power = self.available_build_power();

    loop {
        let outcome = project.tick(&mut self.state, self.dt);
        self.current_time += self.dt;

        if outcome.is_completed() {
            break;
        }

        if self.current_time > 1_000_000.0 {
            panic!("simulation exceeded time limit while building {}", target.id);
        }
    }

    BuildEvent {
        time: self.current_time,
        unit_id: target.id.clone(),
        unit_name: target.name().map(|s| s.to_string()),
    }
}
```

## 3. Example: default chain to Monkeylord

The CLI uses the standard land-tech chain plus the target:

```bash
$ cargo run --bin faf-sim -- simulate -c monkeylord
Simulate target: Cybran Monkeylord (URL0402)

Timeline:
  Time (s)  Unit
  --------  ----
     300.0  T1 Land Factory (URB0101)
    2750.0  T2 Land Factory HQ (URB0201)
   15000.0  T3 Land Factory HQ (URB0301)
   16560.0  T3 Engineer (URL0309)
   44392.3  Monkeylord (URL0402)
```

The exact numbers depend on the current data and whether the build stalls. The
key observation: the baseline is dominated by the expensive T3 factory and the
Monkeylord itself, because we are building them sequentially with the ACU alone
until each prerequisite is done.

## 4. Limitations of the baseline

- **No concurrent building.** We cannot build engineers while the T3 factory is
  being built to speed up later steps.
- **No economy buildings.** Mass extractors and power generators are ignored.
- **All BP on one project.** Even if we own multiple engineers, they all work on
  the same thing in sequence.

These limitations are intentional: they isolate the economy math before we add
planning complexity.

## 5. Study questions

1. Why is the T3 factory completion time roughly `12000 / 10 = 1200` seconds but
may be longer in practice?
2. The T3 engineer is built by the T3 factory (`BuildRate = 90`). Why does it
not finish in `1560 / 90 ≈ 17.3` seconds in the baseline?
3. What is the first project in the chain that is likely to stall? Which
resource runs out first?

## 6. Experiment

Simulate different factions and targets:

```bash
cargo run --bin faf-sim -- simulate -u fatboy
cargo run --bin faf-sim -- simulate -a galacticcolossus
```

Try editing the `standard_tech_chain` in `apps/faf-sim-cli/src/main.rs` to skip
or reorder prerequisites and observe the failure mode.

Next: [04-economy-bottlenecks.md](./04-economy-bottlenecks.md)
