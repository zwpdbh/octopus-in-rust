# 06 — Concurrent Building and BP Allocation

Real FAF games run multiple projects at once: factories build units, engineers
assist, and the ACU builds infrastructure. This document explains how
`HeuristicSimulator` models concurrency.

---

## 1. From one project to many

`SimpleSimulator` (covered in [03-sequential-baseline.md](./03-sequential-baseline.md))
builds one unit at a time. `HeuristicSimulator` instead maintains a set of active
projects and allocates build power among them each tick.

```rust
// crates/faf-sim/src/heuristic.rs ~line 331 — HeuristicSimulator
pub struct HeuristicSimulator<'a, P: BuildPolicy> {
    pub index: &'a DataIndex,
    pub graph: BuildGraph<'a>,
    pub owned_units: Vec<&'a Unit>,
    pub state: EconomyState,
    pub current_time: f64,
    pub dt: f64,
    pub active_projects: Vec<ActiveProject>,
    pub events: Vec<BuildEvent>,
    pub goal: &'a Unit,
    pub policy: P,
}
```

## 2. Active projects

Each active project tracks its target, requested BP, remaining work, and why it
was started:

```rust
// crates/faf-sim/src/heuristic.rs ~line 27 — ActiveProject
pub struct ActiveProject {
    pub target_id: String,
    pub requested_bp: RequestedBuildPower,
    pub project: BuildProject,
    pub priority: ProjectPriority,
}
```

Priorities separate the goal from support units:

```rust
// crates/faf-sim/src/heuristic.rs ~line 17 — ProjectPriority
pub enum ProjectPriority {
    Goal,
    Builder,
    Economy,
}
```

## 3. Proportional BP allocation

Available BP is split proportionally to each project's requested BP:

```rust
// crates/faf-sim/src/heuristic.rs ~line 449 — allocation in tick
let total_available = self.available_bp().0;
let total_requested: f64 = self.active_projects.iter().map(|p| p.requested_bp.0).sum();
let allocation_factor = if total_requested > 0.0 {
    (total_available / total_requested).min(1.0)
} else {
    0.0
};

for project in &mut self.active_projects {
    let allocated = project.requested_bp.0 * allocation_factor;
    project.project.assigned_build_power = RequestedBuildPower(allocated);
}
```

If total requested BP is less than available BP, every project runs at full
requested power. Otherwise they share proportionally.

## 4. Sequential ticking is an approximation

The simulator ticks projects one after another, each updating the shared economy
state:

```rust
// crates/faf-sim/src/heuristic.rs ~line 466 — tick active projects
for i in 0..self.active_projects.len() {
    self.active_projects[i]
        .project
        .tick(&mut self.state, self.dt);
}
```

This is a slight approximation: in the real game all projects drain
simultaneously. The error is small for typical `dt` values and goes to zero as
`dt → 0`.

## 5. Study questions

1. Why allocate BP proportionally rather than giving the goal project everything?
2. What happens if two projects together request more BP than is available?
3. When is the sequential-tick approximation least accurate?

## 6. Experiment

Look at the existing test that compares heuristic vs. baseline for the
Monkeylord:

```rust
// crates/faf-sim/src/heuristic.rs ~line 521 — heuristic_faster_than_acu_alone_for_monkeylord
#[test]
fn heuristic_faster_than_acu_alone_for_monkeylord() {
    let index = load_index();
    let acu = index.find_unit("URL0001").expect("ACU exists");
    let monkeylord = index.find_unit("URL0402").expect("Monkeylord exists");

    let mut baseline = crate::sim::SimpleSimulator::new(&index, vec![acu], 1.0);
    let baseline_events = baseline.simulate_sequence(&[monkeylord]);
    let baseline_time = baseline_events[0].time;

    let mut heuristic = HeuristicSimulator::new(
        &index,
        vec![acu],
        monkeylord,
        GreedyNoStallPolicy::default(),
        1.0,
    );
    let goal_event = heuristic.run().expect("heuristic should finish");

    assert!(
        goal_event.time < baseline_time,
        "heuristic ({}) should beat ACU-alone baseline ({})",
        goal_event.time,
        baseline_time
    );
}
```

Run it with:

```bash
cargo test -p faf-sim heuristic_faster_than_acu_alone_for_monkeylord -- --nocapture
```

Next: [07-greedy-planner.md](./07-greedy-planner.md)
