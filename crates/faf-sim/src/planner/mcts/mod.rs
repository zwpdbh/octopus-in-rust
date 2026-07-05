//! Monte Carlo Tree Search planner guided by a learned direction-only policy.
//!
//! The concrete one-step planner lives in [`direction_planner`]; the learned direction
//! network lives in [`macro_net`]; the heuristic that turns directions into
//! concrete actions lives in [`heuristic`]; the abstract value-net interface
//! lives in [`value_net`]; training lives in [`train`].

pub mod direction_planner;
pub mod features;
pub mod heuristic;
pub mod macro_net;
pub mod search;
pub mod train;
pub mod value_net;

pub use direction_planner::plan;
pub use value_net::{MlpValueNet, ValueNet, ValueNetFactory};
