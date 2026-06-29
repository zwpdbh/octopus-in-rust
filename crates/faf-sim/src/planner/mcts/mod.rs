//! Monte Carlo Tree Search planner guided by a learned hierarchical policy.
//!
//! The concrete one-step planner lives in [`policy`]; the three learned
//! networks live in [`macro_net`]; training lives in [`train`].

pub mod features;
pub mod macro_net;
pub mod policy;
pub mod search;
pub mod selections;
pub mod train;

pub use policy::plan;
