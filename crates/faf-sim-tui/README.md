# faf-sim-tui

Terminal dashboard for [`faf-sim`](../../crates/faf-sim) policy training.

## Purpose

`faf-sim-tui` provides a lightweight [ratatui](https://ratatui.rs/) dashboard that
visualizes live REINFORCE training progress. It is kept as a separate crate so
that the core simulator does not depend on any UI library.

## What it shows

- Overall progress (current episode / total episodes, elapsed time, ETA)
- Best completion time seen so far
- Recent rolling metrics: goal-reach rate, average loss, average steps, epsilon
- Loss and completion-time sparklines
- Greedy evaluation history
- Fine-tuning progress

## Usage

The `faf-sim-cli` binary uses the dashboard automatically for the `train`
subcommand when stdout is a terminal. Pass `--no-tui` to keep plain-text output,
or `--quiet` to suppress all live output.

Programmatically, wrap a training closure with [`TrainingDashboard::run`]:

```rust
use faf_sim::planner::mcts::train::{train_policy, TrainConfig};
use faf_sim_tui::TrainingDashboard;

let (model, best_model, stats) = TrainingDashboard::run(|observer| {
    train_policy(&units, &goal, TrainConfig::default(), observer)
});
```

Press `q` or `Esc` while the dashboard is visible to request a graceful stop at
the next episode boundary.
