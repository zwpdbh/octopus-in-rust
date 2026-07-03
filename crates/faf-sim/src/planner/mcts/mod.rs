//! Monte Carlo Tree Search planner guided by a learned direction-only policy.
//!
//! The concrete one-step planner lives in [`policy`]; the learned direction
//! network lives in [`macro_net`]; the heuristic that turns directions into
//! concrete actions lives in [`heuristic`]; the abstract value-net interface
//! lives in [`value_net`]; training lives in [`train`].

pub mod features;
pub mod heuristic;
pub mod macro_net;
pub mod policy;
pub mod search;
pub mod selections;
pub mod train;
pub mod value_net;

pub use policy::plan;
pub use value_net::{MlpValueNet, ValueNet, ValueNetFactory};
