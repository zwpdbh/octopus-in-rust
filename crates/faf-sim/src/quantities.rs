//! Strongly-typed resource quantities for FAF build-order simulation.
//!
//! This module wraps raw `f64` values in domain-specific newtypes so that mass,
//! energy, time, build power, and their rates cannot be accidentally mixed at
//! compile time.

use serde::{Deserialize, Serialize};
use std::ops::{Add, Div, Mul, Sub};

macro_rules! quantity {
    ($name:ident) => {
        #[derive(Debug, Default, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
        pub struct $name(f64);

        impl $name {
            /// Create a quantity from a raw `f64` value.
            pub const fn from_raw(value: f64) -> Self {
                Self(value)
            }

            /// Return the underlying raw value.
            pub const fn value(self) -> f64 {
                self.0
            }

            /// Return a zero-valued quantity.
            pub const fn zero() -> Self {
                Self(0.0)
            }

            /// Return the maximum of two quantities.
            pub fn max(self, other: Self) -> Self {
                Self(self.0.max(other.0))
            }

            /// Return the minimum of two quantities.
            pub fn min(self, other: Self) -> Self {
                Self(self.0.min(other.0))
            }

            /// Clamp the quantity between two bounds.
            pub fn clamp(self, min: Self, max: Self) -> Self {
                Self(self.0.clamp(min.0, max.0))
            }

            /// Return the absolute value.
            pub fn abs(self) -> Self {
                Self(self.0.abs())
            }
        }

        impl PartialEq<f64> for $name {
            fn eq(&self, other: &f64) -> bool {
                self.0 == *other
            }
        }

        impl PartialOrd<f64> for $name {
            fn partial_cmp(&self, other: &f64) -> Option<std::cmp::Ordering> {
                self.0.partial_cmp(other)
            }
        }

        impl PartialEq<$name> for f64 {
            fn eq(&self, other: &$name) -> bool {
                *self == other.0
            }
        }

        impl PartialOrd<$name> for f64 {
            fn partial_cmp(&self, other: &$name) -> Option<std::cmp::Ordering> {
                self.partial_cmp(&other.0)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

quantity!(Mass);
quantity!(Energy);
quantity!(Time);
quantity!(BuildPower);
quantity!(BuildWork);
quantity!(MassRate);
quantity!(EnergyRate);

/// A storage container pairing the currently held amount with its capacity.
///
/// Used for mass/energy storage in the economy model. Keeping `current` and
/// `cap` together prevents mixing up the held amount with the maximum capacity.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Storage<T> {
    pub current: T,
    pub cap: T,
}

impl<T> Storage<T> {
    /// Create a storage container with the given current amount and capacity.
    pub const fn new(current: T, cap: T) -> Self {
        Self { current, cap }
    }
}

// ---- Addition and subtraction for quantities of the same kind ----

macro_rules! impl_add_sub {
    ($type:ident) => {
        impl Add for $type {
            type Output = Self;
            fn add(self, rhs: Self) -> Self {
                Self(self.0 + rhs.0)
            }
        }

        impl Sub for $type {
            type Output = Self;
            fn sub(self, rhs: Self) -> Self {
                Self(self.0 - rhs.0)
            }
        }
    };
}

impl_add_sub!(Mass);
impl_add_sub!(Energy);
impl_add_sub!(Time);
impl_add_sub!(BuildPower);
impl_add_sub!(BuildWork);
impl_add_sub!(MassRate);
impl_add_sub!(EnergyRate);

// ---- Multiplication: rate * time = amount ----

impl Mul<Time> for MassRate {
    type Output = Mass;
    fn mul(self, rhs: Time) -> Mass {
        Mass(self.0 * rhs.0)
    }
}

impl Mul<Time> for EnergyRate {
    type Output = Energy;
    fn mul(self, rhs: Time) -> Energy {
        Energy(self.0 * rhs.0)
    }
}

impl Mul<Time> for BuildPower {
    type Output = BuildWork;
    fn mul(self, rhs: Time) -> BuildWork {
        BuildWork(self.0 * rhs.0)
    }
}

// ---- Division: amount / time = rate ----

impl Div<Time> for Mass {
    type Output = MassRate;
    fn div(self, rhs: Time) -> MassRate {
        MassRate(self.0 / rhs.0)
    }
}

impl Div<Time> for Energy {
    type Output = EnergyRate;
    fn div(self, rhs: Time) -> EnergyRate {
        EnergyRate(self.0 / rhs.0)
    }
}

// ---- Division: work / power = time ----

impl Div<BuildPower> for BuildWork {
    type Output = Time;
    fn div(self, rhs: BuildPower) -> Time {
        Time(self.0 / rhs.0)
    }
}

impl Div<Time> for BuildWork {
    type Output = BuildPower;
    fn div(self, rhs: Time) -> BuildPower {
        BuildPower(self.0 / rhs.0)
    }
}

// ---- Scalar division for ratios ----

impl Div for Mass {
    type Output = f64;
    fn div(self, rhs: Self) -> f64 {
        self.0 / rhs.0
    }
}

impl Div for Energy {
    type Output = f64;
    fn div(self, rhs: Self) -> f64 {
        self.0 / rhs.0
    }
}

impl Div for BuildWork {
    type Output = f64;
    fn div(self, rhs: Self) -> f64 {
        self.0 / rhs.0
    }
}

impl Div for Time {
    type Output = f64;
    fn div(self, rhs: Self) -> f64 {
        self.0 / rhs.0
    }
}

impl Div for MassRate {
    type Output = f64;
    fn div(self, rhs: Self) -> f64 {
        self.0 / rhs.0
    }
}

impl Div for EnergyRate {
    type Output = f64;
    fn div(self, rhs: Self) -> f64 {
        self.0 / rhs.0
    }
}

// ---- Scalar multiplication for scaling ----

impl Mul<f64> for Mass {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self(self.0 * rhs)
    }
}

impl Mul<f64> for Energy {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self(self.0 * rhs)
    }
}

impl Mul<f64> for BuildPower {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self(self.0 * rhs)
    }
}

impl Mul<f64> for BuildWork {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self(self.0 * rhs)
    }
}

impl Mul<f64> for MassRate {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self(self.0 * rhs)
    }
}

impl Mul<f64> for EnergyRate {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self(self.0 * rhs)
    }
}

impl Mul<Mass> for f64 {
    type Output = Mass;
    fn mul(self, rhs: Mass) -> Mass {
        Mass(self * rhs.0)
    }
}

impl Mul<Energy> for f64 {
    type Output = Energy;
    fn mul(self, rhs: Energy) -> Energy {
        Energy(self * rhs.0)
    }
}

impl Mul<BuildPower> for f64 {
    type Output = BuildPower;
    fn mul(self, rhs: BuildPower) -> BuildPower {
        BuildPower(self * rhs.0)
    }
}

impl Mul<BuildWork> for f64 {
    type Output = BuildWork;
    fn mul(self, rhs: BuildWork) -> BuildWork {
        BuildWork(self * rhs.0)
    }
}

impl Mul<MassRate> for f64 {
    type Output = MassRate;
    fn mul(self, rhs: MassRate) -> MassRate {
        MassRate(self * rhs.0)
    }
}

impl Mul<EnergyRate> for f64 {
    type Output = EnergyRate;
    fn mul(self, rhs: EnergyRate) -> EnergyRate {
        EnergyRate(self * rhs.0)
    }
}

// ---- Negation ----

impl std::ops::Neg for MassRate {
    type Output = Self;
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

impl std::ops::Neg for EnergyRate {
    type Output = Self;
    fn neg(self) -> Self {
        Self(-self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mass_rate_times_time_is_mass() {
        let rate = MassRate::from_raw(10.0);
        let time = Time::from_raw(5.0);
        assert_eq!((rate * time).value(), 50.0);
    }

    #[test]
    fn build_power_times_time_is_work() {
        let power = BuildPower::from_raw(20.0);
        let time = Time::from_raw(3.0);
        assert_eq!((power * time).value(), 60.0);
    }

    #[test]
    fn work_divided_by_power_is_time() {
        let work = BuildWork::from_raw(60.0);
        let power = BuildPower::from_raw(20.0);
        assert_eq!((work / power).value(), 3.0);
    }

    #[test]
    fn same_kind_quantities_add_and_subtract() {
        let a = Mass::from_raw(10.0);
        let b = Mass::from_raw(3.0);
        assert_eq!((a - b).value(), 7.0);
    }

    #[test]
    fn scalar_ratio_is_raw_f64() {
        let a = Mass::from_raw(10.0);
        let b = Mass::from_raw(2.0);
        assert_eq!(a / b, 5.0);
    }
}
