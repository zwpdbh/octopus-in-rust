//! Terminal dashboard for `faf-sim` policy training.
//!
//! The dashboard runs in its own thread and receives coarse-grained training
//! events from the main training loop via [`DashboardObserver`]. Use
//! [`TrainingDashboard::run`] to execute a training closure while the TUI is
//! active.

mod dashboard;
mod observer;

pub use dashboard::TrainingDashboard;
pub use observer::DashboardObserver;
