//! Generic training TUI renderer for Burn.
//!
//! Provides a terminal dashboard similar to Burn's built-in renderer, but with
//! a layout optimized for smaller terminals: the status panel is removed and
//! the metrics text panel is expanded.

pub mod renderer;

pub use renderer::TrainTuiRenderer;
