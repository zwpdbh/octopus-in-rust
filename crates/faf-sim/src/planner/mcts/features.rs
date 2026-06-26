//! State featurization for the value network.
//!
//! Converts a variable-size `GraphState` into a fixed-size `Vec<f32>` that the
//! MLP can consume. This is the bridge between the simulator and the neural
//! network.

use crate::sim::GraphState;
use crate::units::Units;

/// Number of features produced by [`featurize`].
///
/// This must match the input dimension of [`super::ValueNet`].
pub const FEATURE_COUNT: usize = 64;

/// Convert a simulator state into a fixed-length feature vector.
///
/// The exact features are not finalized; this function returns a zero-padded
/// placeholder so the value-network shape can be wired up and compiled.
pub fn featurize(_state: &GraphState, _goal_id: &str, _units: &Units) -> Vec<f32> {
    todo!("state featurization is not yet implemented")
}
