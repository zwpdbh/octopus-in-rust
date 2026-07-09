//! Discrete simulation tick.
//!
//! RTS games advance the world in fixed simulation ticks rather than by wall-clock
//! time. `GameTick` is the unit of that timeline.

/// A single step on the simulation timeline.
///
/// Ticks are opaque unsigned integers. They can be compared and incremented, but
/// converting them to seconds requires the engine's fixed `ticks_per_second`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GameTick(pub u64);

impl GameTick {
    /// The first tick of a simulation.
    pub const FIRST: Self = Self(0);

    /// Return the next tick.
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }

    /// Return the tick `n` steps ahead.
    ///
    /// Panics on overflow in debug builds.
    pub fn advance(self, n: u64) -> Self {
        Self(self.0 + n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_advances() {
        let t = GameTick::FIRST;
        assert_eq!(t.next(), GameTick(1));
        assert_eq!(t.advance(5), GameTick(5));
    }
}
