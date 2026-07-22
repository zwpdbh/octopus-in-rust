//! Eco scheduling mode plugin.

pub mod decide_direction;
pub mod evaluate;
pub mod generate;
pub mod observe;

mod plugin;
pub use plugin::EcoSchedulingPlugin;
