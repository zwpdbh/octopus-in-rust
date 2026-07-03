//! Economy formulas for FAF build-order simulation.
//!
//! Supreme Commander / FAF uses a continuous-drain build model:
//!
//! - A project consumes mass and energy every second while it is being built.
//! - The consumption rate scales with the build power assigned to the project.
//! - If available income cannot cover the drain, the project stalls and its
//!   effective build power drops.
//!
//! This module provides the pure math for computing drain rates and stall
//! factors. It does not simulate queues, concurrent projects, or economy
//! growth — that belongs in the simulator layer.

use crate::units::{UnitCost, UnitDef, UnitKind, Units};
use faf_units::BuildTargetStats;

/// Build power requested for a project, before any stall adjustment.
///
/// This is the sum of `BuildRate` values from engineers, factories, or other
/// builders assigned to the project.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct RequestedBuildPower(pub f64);

impl RequestedBuildPower {
    /// Create a requested build power value.
    ///
    /// Returns `None` if the value is not positive.
    pub fn new(value: f64) -> Option<Self> {
        if value > 0.0 {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Convert to effective build power given a stall factor in `[0, 1]`.
    pub fn to_effective(self, stall_factor: f64) -> EffectiveBuildPower {
        EffectiveBuildPower(self.0 * stall_factor.clamp(0.0, 1.0))
    }
}

/// Build power actually applied to a project after stall adjustment.
///
/// This is always less than or equal to the requested power.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct EffectiveBuildPower(pub f64);

impl EffectiveBuildPower {
    /// Create an effective build power value.
    ///
    /// Returns `None` if the value is negative.
    pub fn new(value: f64) -> Option<Self> {
        if value >= 0.0 {
            Some(Self(value))
        } else {
            None
        }
    }
}

/// Drain rates and progress for a single project.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BuildDrain {
    /// Mass consumed per second while building.
    pub mass_per_second: f64,
    /// Energy consumed per second while building.
    pub energy_per_second: f64,
    /// Fraction of the project completed per second (0..1).
    pub progress_per_second: f64,
    /// Total mass required to finish the project.
    pub total_mass: f64,
    /// Total energy required to finish the project.
    pub total_energy: f64,
    /// Total build time at the assigned build power, ignoring stalls.
    pub completion_time_seconds: f64,
    /// Build power used for this calculation.
    pub assigned_build_power: RequestedBuildPower,
}

/// Compute drain rates for building a target with a given amount of build power.
///
/// Returns `None` if the target's build time is zero.
pub fn compute_drain(
    target: &BuildTargetStats,
    assigned_build_power: RequestedBuildPower,
) -> Option<BuildDrain> {
    let build_time = target.build_time;
    let total_mass = target.build_cost_mass;
    let total_energy = target.build_cost_energy;

    if build_time <= 0.0 {
        return None;
    }

    // In FAF, progress is proportional to assigned build power:
    //   progress_per_second = build_power / BuildTime
    // This means a unit with BuildTime 100 and build power 10 takes 10s.
    let power = assigned_build_power.0;
    let progress_per_second = power / build_time;
    let completion_time_seconds = 1.0 / progress_per_second;

    // Resource drain scales with the same factor:
    //   drain_per_second = progress_per_second * total_cost
    //                    = (build_power / BuildTime) * BuildCost
    let mass_per_second = progress_per_second * total_mass;
    let energy_per_second = progress_per_second * total_energy;

    Some(BuildDrain {
        mass_per_second,
        energy_per_second,
        progress_per_second,
        total_mass,
        total_energy,
        completion_time_seconds,
        assigned_build_power,
    })
}

/// Sum the build power of real builders: commanders, engineers, and factories.
///
/// Only units with a positive `build_rate` contribute. In the strongly-typed
/// model the only such kinds are commanders, engineers, and factories, so no
/// additional category filter is needed.
pub fn total_build_power(units: &Units, builders: &[UnitKind]) -> RequestedBuildPower {
    RequestedBuildPower(
        builders
            .iter()
            .filter_map(|kind| units.def(kind))
            .filter(|def| def.build_rate() > 0.0)
            .map(|def| def.build_rate())
            .sum(),
    )
}

/// A directed flow of mass and energy per second.
///
/// Positive values represent resources entering the economy; negative values
/// represent resources leaving it. This matches what a player observes in the
/// in-game economy overlay.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EcoFlow {
    pub mass_per_second: f64,
    pub energy_per_second: f64,
}

impl EcoFlow {
    /// Zero flow.
    pub const ZERO: Self = Self {
        mass_per_second: 0.0,
        energy_per_second: 0.0,
    };

    /// Net flow: production minus consumption.
    pub fn net(production: &Self, consumption: &Self) -> Self {
        Self {
            mass_per_second: production.mass_per_second - consumption.mass_per_second,
            energy_per_second: production.energy_per_second - consumption.energy_per_second,
        }
    }
}

impl std::ops::Add for EcoFlow {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self {
            mass_per_second: self.mass_per_second + rhs.mass_per_second,
            energy_per_second: self.energy_per_second + rhs.energy_per_second,
        }
    }
}

impl std::ops::Sub for EcoFlow {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self {
            mass_per_second: self.mass_per_second - rhs.mass_per_second,
            energy_per_second: self.energy_per_second - rhs.energy_per_second,
        }
    }
}

impl std::iter::Sum for EcoFlow {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, |a, b| a + b)
    }
}

/// Something that adds mass and/or energy to the economy each second.
pub trait EcoProducer {
    fn production(&self) -> EcoFlow;
}

/// Something that removes mass and/or energy from the economy each second.
pub trait EcoConsumer {
    fn consumption(&self) -> EcoFlow;
}

impl EcoConsumer for BuildProject {
    fn consumption(&self) -> EcoFlow {
        compute_drain(&self.target.to_target_stats(), self.assigned_build_power).map_or(
            EcoFlow::ZERO,
            |d| EcoFlow {
                mass_per_second: d.mass_per_second,
                energy_per_second: d.energy_per_second,
            },
        )
    }
}

/// A unified view of any unit that produces mass or energy.
///
/// Mass extractors, power generators, and their upgrades all expose the same
/// economic metrics (build cost, build time, production, maintenance). This
/// view lets the planner treat them uniformly when making efficiency-aware
/// decisions.
#[derive(Debug, Clone, Copy)]
pub struct ResourceProducer<'a> {
    def: &'a UnitDef,
}

impl<'a> ResourceProducer<'a> {
    /// Create a producer view for `kind`.
    ///
    /// Returns `None` if the unit has no definition, zero mass cost, or no
    /// resource production.
    pub fn new(units: &'a Units, kind: &'a UnitKind) -> Option<Self> {
        let def = units.def(kind)?;
        if def.cost.mass <= 0.0 {
            return None;
        }
        let prod = def.production();
        if prod.mass_per_second <= 0.0 && prod.energy_per_second <= 0.0 {
            return None;
        }
        Some(Self { def })
    }

    /// Build stats of this producer.
    pub fn stats(&self) -> UnitCost {
        self.def.cost
    }

    /// Gross production flow.
    pub fn production(&self) -> EcoFlow {
        self.def.production()
    }

    /// Maintenance consumption flow.
    pub fn maintenance(&self) -> EcoFlow {
        self.def.consumption()
    }

    /// Net production after maintenance.
    pub fn net_production(&self) -> EcoFlow {
        self.production() - self.maintenance()
    }

    /// Mass income per mass invested.
    pub fn mass_efficiency(&self) -> f64 {
        self.production().mass_per_second / self.def.cost.mass
    }

    /// Energy income per mass invested.
    pub fn energy_efficiency(&self) -> f64 {
        self.production().energy_per_second / self.def.cost.mass
    }
}

/// Summarize the net mass and energy flow for a collection of owned units and
/// active construction projects.
///
/// The returned flow can be negative when consumption exceeds production, just
/// like the in-game economy display during heavy construction.
pub fn summarize_economy(
    units: &Units,
    owned: &[UnitKind],
    active_projects: &[&BuildProject],
) -> EcoFlow {
    let production: EcoFlow = owned
        .iter()
        .filter_map(|kind| units.def(kind))
        .map(|def| def.production())
        .sum();
    let maintenance: EcoFlow = owned
        .iter()
        .filter_map(|kind| units.def(kind))
        .map(|def| def.consumption())
        .sum();
    let construction: EcoFlow = active_projects.iter().map(|p| p.consumption()).sum();
    production - maintenance - construction
}

/// Current economy state at a point in time.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EconomyState {
    /// Mass income per second (can be negative during drains).
    pub net_mass_income: f64,
    /// Energy income per second (can be negative during drains).
    pub net_energy_income: f64,
    /// Mass currently in storage.
    pub mass_storage: f64,
    /// Energy currently in storage.
    pub energy_storage: f64,
    /// Maximum mass storage capacity.
    pub mass_storage_cap: f64,
    /// Maximum energy storage capacity.
    pub energy_storage_cap: f64,
}

impl EconomyState {
    /// Estimate how long it would take this economy to finish `cost` work at
    /// `build_power` under a continuous, constant-income approximation.
    ///
    /// Unlike taking the max of independent mass/energy/build estimates, this
    /// models the interaction between them: resources drain continuously while
    /// building, so a resource shortage can force build power to throttle even
    /// before storage is empty.
    ///
    /// Assumptions:
    /// - Mass and energy income stay constant.
    /// - Total build power stays constant.
    /// - Remaining resource costs are distributed proportionally over remaining
    ///   build work (a continuous / fluid approximation).
    /// - Resources already in storage can be spent immediately.
    ///
    /// The result is exact under those assumptions. In the real planner, income
    /// and build power change as new units are built, and projects are discrete,
    /// so this remains a heuristic estimate.
    pub fn estimate_remaining_time(&self, cost: BuildTargetStats, build_power: f64) -> f64 {
        if cost.build_time <= 0.0 {
            return 0.0;
        }
        if build_power <= 0.0 {
            return f64::INFINITY;
        }

        let mut remaining_mass = cost.build_cost_mass;
        let mut remaining_energy = cost.build_cost_energy;
        let mut remaining_work = cost.build_time;
        let mut mass_storage = self.mass_storage;
        let mut energy_storage = self.energy_storage;
        let mass_income = self.net_mass_income;
        let energy_income = self.net_energy_income;
        let mut elapsed = 0.0;

        while remaining_work > 1e-9 {
            // Cost intensity of the remaining work (fluid approximation).
            let mass_per_work = remaining_mass / remaining_work;
            let energy_per_work = remaining_energy / remaining_work;

            // Sustainable build rate for each resource (income == drain).
            let mass_sustainable_bp = if mass_per_work > 0.0 {
                mass_income / mass_per_work
            } else {
                f64::INFINITY
            };
            let energy_sustainable_bp = if energy_per_work > 0.0 {
                energy_income / energy_per_work
            } else {
                f64::INFINITY
            };

            // Effective BP is limited by resources whose storage is already empty:
            // we cannot drain them faster than income allows.
            let mut effective_bp = build_power;
            if mass_storage <= 1e-9 {
                effective_bp = effective_bp.min(mass_sustainable_bp);
            }
            if energy_storage <= 1e-9 {
                effective_bp = effective_bp.min(energy_sustainable_bp);
            }

            if effective_bp <= 1e-9 {
                return f64::INFINITY;
            }

            // Build at effective_bp. How long until a resource with positive
            // storage depletes?
            let mass_drain_rate = effective_bp * mass_per_work;
            let energy_drain_rate = effective_bp * energy_per_work;
            let net_mass = mass_income - mass_drain_rate;
            let net_energy = energy_income - energy_drain_rate;

            let time_to_finish = remaining_work / effective_bp;
            let mut dt = time_to_finish;
            if mass_storage > 1e-9 && net_mass < -1e-9 {
                dt = dt.min(-mass_storage / net_mass);
            }
            if energy_storage > 1e-9 && net_energy < -1e-9 {
                dt = dt.min(-energy_storage / net_energy);
            }

            if dt <= 1e-9 {
                // Cannot make progress; prevent an infinite loop.
                return f64::INFINITY;
            }

            elapsed += dt;
            mass_storage += net_mass * dt;
            energy_storage += net_energy * dt;
            remaining_work -= effective_bp * dt;
            remaining_mass -= mass_drain_rate * dt;
            remaining_energy -= energy_drain_rate * dt;
        }

        elapsed
    }
}

/// Result of applying a drain to an economy state for one second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TickResult {
    /// Effective build power after considering stalls.
    pub effective_build_power: EffectiveBuildPower,
    /// Mass actually consumed.
    pub mass_consumed: f64,
    /// Energy actually consumed.
    pub energy_consumed: f64,
    /// New mass storage after the tick.
    pub new_mass_storage: f64,
    /// New energy storage after the tick.
    pub new_energy_storage: f64,
    /// True if energy stalled during the tick.
    pub energy_stalled: bool,
    /// True if mass stalled during the tick.
    pub mass_stalled: bool,
}

/// Compute the effective build power when the requested drain exceeds
/// available resources.
///
/// FAF stalls when storage would go negative. The effective build power is
/// reduced to the largest fraction of the requested power that keeps both
/// resources non-negative.
pub fn apply_tick(requested: &BuildDrain, state: &EconomyState, dt: f64) -> TickResult {
    // Gross income during this tick, ignoring the drain.
    let mass_income = state.net_mass_income * dt;
    let energy_income = state.net_energy_income * dt;

    // Requested consumption over the tick.
    let requested_mass = requested.mass_per_second * dt;
    let requested_energy = requested.energy_per_second * dt;

    // Maximum sustainable fraction of requested drain before each resource
    // would hit zero. If income already covers drain, the factor is 1.0.
    let mass_factor = if requested_mass <= 0.0 {
        1.0
    } else {
        let available = (state.mass_storage + mass_income).max(0.0);
        (available / requested_mass).min(1.0)
    };

    let energy_factor = if requested_energy <= 0.0 {
        1.0
    } else {
        let available = (state.energy_storage + energy_income).max(0.0);
        (available / requested_energy).min(1.0)
    };

    // The project can only run as fast as the most constrained resource.
    let effective_factor = mass_factor.min(energy_factor);
    let effective_build_power = requested
        .assigned_build_power
        .to_effective(effective_factor);

    let mass_consumed = requested_mass * effective_factor;
    let energy_consumed = requested_energy * effective_factor;

    let new_mass_storage = (state.mass_storage + mass_income - mass_consumed)
        .min(state.mass_storage_cap)
        .max(0.0);
    let new_energy_storage = (state.energy_storage + energy_income - energy_consumed)
        .min(state.energy_storage_cap)
        .max(0.0);

    TickResult {
        effective_build_power,
        mass_consumed,
        energy_consumed,
        new_mass_storage,
        new_energy_storage,
        energy_stalled: effective_factor < 1.0 && energy_factor <= mass_factor,
        mass_stalled: effective_factor < 1.0 && mass_factor <= energy_factor,
    }
}

/// Result of applying a *global* drain to an economy state for one tick.
///
/// Unlike [`TickResult`], this models the combined drain of all active
/// projects and includes the graph-model assumption that energy stall also
/// scales mass income.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GraphTickResult {
    /// Global stall factor applied to every active project.
    pub effective_factor: f64,
    /// Mass actually consumed across all projects.
    pub mass_consumed: f64,
    /// Energy actually consumed across all projects.
    pub energy_consumed: f64,
    /// New mass storage after the tick.
    pub new_mass_storage: f64,
    /// New energy storage after the tick.
    pub new_energy_storage: f64,
    /// True if energy was the limiting resource.
    pub energy_stalled: bool,
    /// True if mass was the limiting resource.
    pub mass_stalled: bool,
    /// Net mass income after applying the energy-stall scaling.
    pub scaled_net_mass_income: f64,
}

/// Compute the effective global stall factor for the combined drain of all
/// active projects in the graph model.
///
/// The graph model assumes that when energy is the limiting resource, mass
/// production is scaled down by the same energy factor. This can in turn make
/// mass the limiting resource, so the factor is recomputed once after scaling.
pub fn apply_tick_graph(
    total_mass_drain: f64,
    total_energy_drain: f64,
    state: &EconomyState,
    dt: f64,
) -> GraphTickResult {
    let mass_income = state.net_mass_income * dt;
    let energy_income = state.net_energy_income * dt;

    let requested_mass = total_mass_drain * dt;
    let requested_energy = total_energy_drain * dt;

    // Energy factor without mass-income scaling.
    let energy_factor = if requested_energy <= 0.0 {
        1.0
    } else {
        let available = (state.energy_storage + energy_income).max(0.0);
        (available / requested_energy).min(1.0)
    };

    // Mass factor before considering energy-driven mass-income reduction.
    let mass_factor_unscaled = if requested_mass <= 0.0 {
        1.0
    } else {
        let available = (state.mass_storage + mass_income).max(0.0);
        (available / requested_mass).min(1.0)
    };

    // If energy is the binding constraint, mass income scales with it.
    let scaled_net_mass_income = if energy_factor <= mass_factor_unscaled {
        state.net_mass_income * energy_factor
    } else {
        state.net_mass_income
    };
    let scaled_mass_income = scaled_net_mass_income * dt;

    // Recompute mass factor with the possibly reduced mass income.
    let mass_factor = if requested_mass <= 0.0 {
        1.0
    } else {
        let available = (state.mass_storage + scaled_mass_income).max(0.0);
        (available / requested_mass).min(1.0)
    };

    let effective_factor = mass_factor.min(energy_factor);

    let mass_consumed = requested_mass * effective_factor;
    let energy_consumed = requested_energy * effective_factor;

    let new_mass_storage = (state.mass_storage + scaled_mass_income - mass_consumed)
        .min(state.mass_storage_cap)
        .max(0.0);
    let new_energy_storage = (state.energy_storage + energy_income - energy_consumed)
        .min(state.energy_storage_cap)
        .max(0.0);

    GraphTickResult {
        effective_factor,
        mass_consumed,
        energy_consumed,
        new_mass_storage,
        new_energy_storage,
        energy_stalled: effective_factor < 1.0 && energy_factor <= mass_factor,
        mass_stalled: effective_factor < 1.0 && mass_factor <= energy_factor,
        scaled_net_mass_income,
    }
}

/// A single project being built.
///
/// Progress is tracked as **remaining work** measured in the target's
/// `BuildTime` units. This is more robust than tracking a percentage because
/// assigned build power can change every tick.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildProject {
    /// Kind of unit being built.
    pub target_kind: UnitKind,
    /// Static build cost of the unit being built.
    pub target: UnitCost,
    /// Total build power currently assigned to this project.
    pub assigned_build_power: RequestedBuildPower,
    /// Remaining work in `BuildTime` units. Starts at `BuildTime`, reaches 0
    /// when complete.
    pub remaining_work: f64,
}

/// Result of ticking a single project.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TickOutcome {
    /// Project is still being built.
    InProgress {
        /// Completion progress at the end of the tick (0.0 to 1.0).
        progress: f64,
        /// Effective build power after stall adjustment.
        effective_build_power: EffectiveBuildPower,
        /// True if energy was the limiting resource.
        energy_stalled: bool,
        /// True if mass was the limiting resource.
        mass_stalled: bool,
    },
    /// Project finished during this tick.
    Completed {
        /// Effective build power after stall adjustment.
        effective_build_power: EffectiveBuildPower,
        /// True if energy was the limiting resource.
        energy_stalled: bool,
        /// True if mass was the limiting resource.
        mass_stalled: bool,
    },
}

impl TickOutcome {
    /// True if the project completed during this tick.
    pub fn is_completed(&self) -> bool {
        matches!(self, TickOutcome::Completed { .. })
    }

    /// Effective build power after stall adjustment, regardless of outcome.
    pub fn effective_build_power(&self) -> EffectiveBuildPower {
        match self {
            TickOutcome::InProgress {
                effective_build_power,
                ..
            }
            | TickOutcome::Completed {
                effective_build_power,
                ..
            } => *effective_build_power,
        }
    }
}

impl BuildProject {
    /// Create a new project for the given unit kind.
    ///
    /// Returns `None` if the unit has no definition or zero build time.
    pub fn new(target: UnitKind, units: &Units) -> Option<Self> {
        let def = units.def(&target)?;
        if def.cost.build_time <= 0.0 {
            return None;
        }
        Some(Self {
            target_kind: target,
            target: def.cost,
            assigned_build_power: RequestedBuildPower(0.0),
            remaining_work: def.cost.build_time,
        })
    }

    /// Current completion progress as a fraction between 0.0 and 1.0.
    pub fn progress(&self) -> f64 {
        let total = self.target.build_time;
        let done = (total - self.remaining_work).max(0.0);
        done / total
    }

    /// True if the project has no remaining work.
    pub fn is_complete(&self) -> bool {
        self.remaining_work <= 0.0
    }

    /// Advance the project by `dt` seconds, consuming resources from `state`.
    pub fn tick(&mut self, state: &mut EconomyState, dt: f64) -> TickOutcome {
        let Some(drain) = compute_drain(&self.target.to_target_stats(), self.assigned_build_power)
        else {
            return TickOutcome::InProgress {
                progress: self.progress(),
                effective_build_power: EffectiveBuildPower(0.0),
                energy_stalled: false,
                mass_stalled: false,
            };
        };

        let tick = apply_tick(&drain, state, dt);

        // Decrement remaining work by effective build power * dt.
        self.remaining_work -= tick.effective_build_power.0 * dt;

        // Update economy state.
        state.mass_storage = tick.new_mass_storage;
        state.energy_storage = tick.new_energy_storage;

        if self.remaining_work <= 0.0 {
            self.remaining_work = 0.0;
            TickOutcome::Completed {
                effective_build_power: tick.effective_build_power,
                energy_stalled: tick.energy_stalled,
                mass_stalled: tick.mass_stalled,
            }
        } else {
            TickOutcome::InProgress {
                progress: self.progress(),
                effective_build_power: tick.effective_build_power,
                energy_stalled: tick.energy_stalled,
                mass_stalled: tick.mass_stalled,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{TechLevel, UnitId, Units};

    fn load_units() -> Units {
        let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
        Units::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn monkeylord_drain_at_base_build_power() {
        let units = load_units();
        let monkeylord = UnitKind::Unique(UnitId("URL0402".to_string()));
        let cost = units.build_cost(&monkeylord).expect("Monkeylord exists");

        // Base build time means build power = 1.0 (the implicit reference rate).
        let drain =
            compute_drain(&cost.to_target_stats(), RequestedBuildPower(1.0)).expect("valid drain");

        assert_eq!(drain.total_mass, 20000.0);
        assert_eq!(drain.total_energy, 260000.0);
        assert_eq!(drain.completion_time_seconds, 27500.0);

        // At base build power, drain per second = total_cost / build_time.
        assert!((drain.mass_per_second - 20000.0 / 27500.0).abs() < 1e-9);
        assert!((drain.energy_per_second - 260000.0 / 27500.0).abs() < 1e-9);
    }

    #[test]
    fn monkeylord_drain_at_t3_engineer_power() {
        let units = load_units();
        let monkeylord = UnitKind::Unique(UnitId("URL0402".to_string()));
        let t3_eng = UnitKind::Engineer(TechLevel::T3);
        let build_power = RequestedBuildPower(units.def(&t3_eng).unwrap().build_rate());

        let drain = compute_drain(
            &units.build_cost(&monkeylord).unwrap().to_target_stats(),
            build_power,
        )
        .expect("valid drain");

        assert_eq!(drain.assigned_build_power, build_power);
        assert!((drain.completion_time_seconds - 27500.0 / build_power.0).abs() < 1e-9);
        assert!(
            (drain.mass_per_second - drain.total_mass / drain.completion_time_seconds).abs() < 1e-6
        );
    }

    #[test]
    fn total_build_power_sums_builders() {
        let units = load_units();
        let acu = UnitKind::Commander;
        let t1_eng = UnitKind::Engineer(TechLevel::T1);
        let t3_eng = UnitKind::Engineer(TechLevel::T3);

        let total = total_build_power(&units, &[acu.clone(), t1_eng.clone(), t3_eng.clone()]);

        let expected = units.def(&acu).unwrap().build_rate()
            + units.def(&t1_eng).unwrap().build_rate()
            + units.def(&t3_eng).unwrap().build_rate();

        assert!((total.0 - expected).abs() < 1e-9);
    }

    #[test]
    fn tick_with_plenty_resources_runs_full_power() {
        let units = load_units();
        let monkeylord = UnitKind::Unique(UnitId("URL0402".to_string()));
        let drain = compute_drain(
            &units.build_cost(&monkeylord).unwrap().to_target_stats(),
            RequestedBuildPower(10.0),
        )
        .expect("valid drain");

        let state = EconomyState {
            net_mass_income: 1000.0,
            net_energy_income: 10000.0,
            mass_storage: 50000.0,
            energy_storage: 500000.0,
            mass_storage_cap: 100000.0,
            energy_storage_cap: 1000000.0,
        };

        let result = apply_tick(&drain, &state, 1.0);
        assert!((result.effective_build_power.0 - 10.0).abs() < 1e-9);
        assert!(!result.energy_stalled);
        assert!(!result.mass_stalled);
    }

    #[test]
    fn tick_stalls_when_energy_insufficient() {
        let units = load_units();
        let monkeylord = UnitKind::Unique(UnitId("URL0402".to_string()));
        let drain = compute_drain(
            &units.build_cost(&monkeylord).unwrap().to_target_stats(),
            RequestedBuildPower(10.0),
        )
        .expect("valid drain");

        // Very little energy income and storage, plenty of mass.
        let state = EconomyState {
            net_mass_income: 1000.0,
            net_energy_income: 0.0,
            mass_storage: 50000.0,
            energy_storage: drain.energy_per_second * 0.5, // only half a second worth
            mass_storage_cap: 100000.0,
            energy_storage_cap: 1000000.0,
        };

        let result = apply_tick(&drain, &state, 1.0);
        assert!(result.effective_build_power.0 < 10.0);
        assert!(result.energy_stalled);
        assert!(!result.mass_stalled);
        assert!(result.new_energy_storage.abs() < 1e-6);
    }

    #[test]
    fn tick_stalls_when_mass_insufficient() {
        let units = load_units();
        let monkeylord = UnitKind::Unique(UnitId("URL0402".to_string()));
        let drain = compute_drain(
            &units.build_cost(&monkeylord).unwrap().to_target_stats(),
            RequestedBuildPower(10.0),
        )
        .expect("valid drain");

        // Very little mass income and storage, plenty of energy.
        let state = EconomyState {
            net_mass_income: 0.0,
            net_energy_income: 1000.0,
            mass_storage: drain.mass_per_second * 0.5, // only half a second worth
            energy_storage: 500000.0,
            mass_storage_cap: 100000.0,
            energy_storage_cap: 1000000.0,
        };

        let result = apply_tick(&drain, &state, 1.0);
        assert!(result.effective_build_power.0 < 10.0);
        assert!(result.mass_stalled);
        assert!(!result.energy_stalled);
        assert!(result.new_mass_storage.abs() < 1e-6);
    }

    #[test]
    fn build_project_completes_with_constant_power() {
        let units = load_units();
        let t1_eng = UnitKind::Engineer(TechLevel::T1);
        let build_power = RequestedBuildPower(units.def(&t1_eng).unwrap().build_rate());

        // Build a T1 engineer with another T1 engineer.
        let mut project = BuildProject::new(t1_eng, &units).expect("valid unit");
        project.assigned_build_power = build_power;

        let mut state = EconomyState {
            net_mass_income: 1000.0,
            net_energy_income: 10000.0,
            mass_storage: 10000.0,
            energy_storage: 100000.0,
            mass_storage_cap: 1000000.0,
            energy_storage_cap: 1000000.0,
        };

        let build_time = project.target.build_time;
        let outcome = project.tick(&mut state, build_time);

        assert!(outcome.is_completed());
        assert!(project.is_complete());
        assert!((project.progress() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn build_project_progress_tracks_remaining_work() {
        let units = load_units();
        let t1_eng = UnitKind::Engineer(TechLevel::T1);
        let build_power = RequestedBuildPower(units.def(&t1_eng).unwrap().build_rate());

        let mut project = BuildProject::new(t1_eng, &units).expect("valid unit");
        let build_time = project.target.build_time;
        project.assigned_build_power = build_power;

        let mut state = EconomyState {
            net_mass_income: 1000.0,
            net_energy_income: 10000.0,
            mass_storage: 10000.0,
            energy_storage: 100000.0,
            mass_storage_cap: 1000000.0,
            energy_storage_cap: 1000000.0,
        };

        // Tick for half the nominal completion time.
        let half_duration = build_time / build_power.0 / 2.0;
        let outcome = project.tick(&mut state, half_duration);
        match outcome {
            TickOutcome::InProgress { progress, .. } => {
                assert!((progress - 0.5).abs() < 1e-9);
            }
            TickOutcome::Completed { .. } => panic!("should not be complete yet"),
        }
        assert!((project.progress() - 0.5).abs() < 1e-9);
        assert!((project.remaining_work - build_time / 2.0).abs() < 1e-9);
    }

    #[test]
    fn build_project_stalls_and_takes_longer() {
        let units = load_units();
        let t1_eng = UnitKind::Engineer(TechLevel::T1);
        let build_power = RequestedBuildPower(units.def(&t1_eng).unwrap().build_rate());

        let mut project = BuildProject::new(t1_eng, &units).expect("valid unit");
        let build_time = project.target.build_time;
        project.assigned_build_power = build_power;

        // No energy income and tiny storage: will stall.
        let mut state = EconomyState {
            net_mass_income: 1000.0,
            net_energy_income: 0.0,
            mass_storage: 10000.0,
            energy_storage: 10.0,
            mass_storage_cap: 1000000.0,
            energy_storage_cap: 1000000.0,
        };

        // Even after the nominal build time, it should not be complete.
        let outcome = project.tick(&mut state, build_time);
        assert!(!outcome.is_completed());
        match outcome {
            TickOutcome::InProgress {
                energy_stalled: true,
                ..
            } => {}
            TickOutcome::InProgress { .. } => panic!("expected energy stall"),
            TickOutcome::Completed { .. } => panic!("should not be complete yet"),
        }
        assert!(project.progress() < 1.0);
    }

    #[test]
    fn storage_overflow_wastes_income() {
        let units = load_units();
        let t1_eng = UnitKind::Engineer(TechLevel::T1);
        let drain = compute_drain(
            &units.build_cost(&t1_eng).unwrap().to_target_stats(),
            RequestedBuildPower(1.0),
        )
        .expect("valid drain");

        let mut state = EconomyState {
            net_mass_income: 1000.0,
            net_energy_income: 10000.0,
            mass_storage: 100.0,
            energy_storage: 1000.0,
            mass_storage_cap: 100.0,
            energy_storage_cap: 1000.0,
        };

        let result = apply_tick(&drain, &state, 1.0);

        // Storage was already at cap, so income above the drain is wasted.
        assert!((result.new_mass_storage - 100.0).abs() < 1e-9);
        assert!((result.new_energy_storage - 1000.0).abs() < 1e-9);
        state.mass_storage = result.new_mass_storage;
        state.energy_storage = result.new_energy_storage;

        // Tick again: since storage did not grow, nothing changes except the drain.
        let result2 = apply_tick(&drain, &state, 1.0);
        assert!((result2.new_mass_storage - 100.0).abs() < 1e-9);
        assert!((result2.new_energy_storage - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn storage_buffers_during_zero_income() {
        let units = load_units();
        let t1_eng = UnitKind::Engineer(TechLevel::T1);
        let build_power = RequestedBuildPower(units.def(&t1_eng).unwrap().build_rate());

        let mut project = BuildProject::new(t1_eng, &units).expect("valid unit");
        let total_mass = project.target.mass;
        let total_energy = project.target.energy;
        let build_time = project.target.build_time;
        project.assigned_build_power = build_power;

        // No income, but enough storage to pay the full cost.
        let mut state = EconomyState {
            net_mass_income: 0.0,
            net_energy_income: 0.0,
            mass_storage: total_mass,
            energy_storage: total_energy,
            mass_storage_cap: total_mass * 2.0,
            energy_storage_cap: total_energy * 2.0,
        };

        let outcome = project.tick(&mut state, build_time);

        assert!(outcome.is_completed());
        assert!(state.mass_storage.abs() < 1e-6);
        assert!(state.energy_storage.abs() < 1e-6);
    }

    #[test]
    fn summarize_economy_computes_net_flow() {
        let units = load_units();

        // ACU alone: produces 1 mass/s and 20 energy/s, no maintenance.
        let net = summarize_economy(&units, &[UnitKind::Commander], &[]);
        assert!((net.mass_per_second - 1.0).abs() < 1e-9);
        assert!((net.energy_per_second - 20.0).abs() < 1e-9);

        // ACU + T1 mex: mex adds 2 mass/s but consumes 2 energy/s maintenance.
        let net = summarize_economy(
            &units,
            &[UnitKind::Commander, UnitKind::Mex(TechLevel::T1)],
            &[],
        );
        assert!((net.mass_per_second - 3.0).abs() < 1e-9);
        assert!((net.energy_per_second - 18.0).abs() < 1e-9);

        // ACU building Monkeylord with all its BP: construction drain can make
        // net energy strongly negative.
        let mut project =
            BuildProject::new(UnitKind::Unique(UnitId("URL0402".to_string())), &units)
                .expect("valid target");
        project.assigned_build_power =
            RequestedBuildPower(units.def(&UnitKind::Commander).unwrap().build_rate());
        let net = summarize_economy(&units, &[UnitKind::Commander], &[&project]);
        assert!(
            net.energy_per_second < 0.0,
            "building Monkeylord should make net energy negative"
        );
    }

    #[test]
    fn resource_producers_share_uniform_metrics() {
        let units = load_units();
        let mut checked = 0;

        for def in units.defs().values() {
            let is_mex = matches!(def.kind, UnitKind::Mex(_));
            let is_pgen = matches!(def.kind, UnitKind::Pgen(_));
            if !is_mex && !is_pgen {
                continue;
            }

            // Special units such as the Aeon Paragon carry the production
            // categories but have no fixed production values. Only enforce the
            // common metric shape on units that actually produce something.
            let prod = def.production();
            let produces_mass = prod.mass_per_second > 0.0;
            let produces_energy = prod.energy_per_second > 0.0;
            if !produces_mass && !produces_energy {
                continue;
            }

            checked += 1;
            assert!(def.cost.mass > 0.0, "{:?} has no mass cost", def.kind);
            assert!(def.cost.energy > 0.0, "{:?} has no energy cost", def.kind);
            assert!(
                def.cost.build_time > 0.0,
                "{:?} has no build time",
                def.kind
            );

            if is_mex {
                assert!(
                    produces_mass,
                    "{:?} is a mex with no mass production",
                    def.kind
                );
            }
            if is_pgen {
                assert!(
                    produces_energy,
                    "{:?} is a pgen with no energy production",
                    def.kind
                );
            }

            // All resource producers should be representable as a ResourceProducer.
            assert!(
                ResourceProducer::new(&units, &def.kind).is_some(),
                "{:?} should be a valid ResourceProducer",
                def.kind
            );
        }

        assert!(checked > 0, "no resource producers found in index");
    }

    fn test_economy(
        mass_storage: f64,
        energy_storage: f64,
        mass_income: f64,
        energy_income: f64,
    ) -> EconomyState {
        EconomyState {
            net_mass_income: mass_income,
            net_energy_income: energy_income,
            mass_storage,
            energy_storage,
            mass_storage_cap: 0.0,
            energy_storage_cap: 0.0,
        }
    }

    #[test]
    fn estimate_build_power_bottleneck() {
        // Resources and income are abundant; only build power limits progress.
        let economy = test_economy(1000.0, 1000.0, 100.0, 100.0);
        let cost = BuildTargetStats {
            build_cost_mass: 100.0,
            build_cost_energy: 100.0,
            build_time: 100.0,
        };
        let t = economy.estimate_remaining_time(cost, 10.0);
        assert!((t - 10.0).abs() < 1e-9, "expected 10s, got {t}");
    }

    #[test]
    fn estimate_resource_bottleneck_with_storage_burn() {
        // BP is high, income is low, and storage covers some initial work.
        // Total cost: 200 mass + 200 energy, work: 200, BP: 10.
        // Storage: 100 mass + 100 energy.
        // Income: 1 mass/s + 1 energy/s.
        // Cost intensity: 1 mass/work, 1 energy/work.
        // Sustainable BP = min(10, 1/1, 1/1) = 1.
        // Burn at BP 10: drain excess = 9 each. Storage lasts 100/9 ≈ 11.11s.
        // Work done in burn: 111.11. Remaining work: 88.89.
        // Sustainable phase: 88.89 / 1 = 88.89s.
        // Total: 100s.
        let economy = test_economy(100.0, 100.0, 1.0, 1.0);
        let cost = BuildTargetStats {
            build_cost_mass: 200.0,
            build_cost_energy: 200.0,
            build_time: 200.0,
        };
        let t = economy.estimate_remaining_time(cost, 10.0);
        assert!((t - 100.0).abs() < 1e-6, "expected ~100s, got {t}");
    }

    #[test]
    fn estimate_no_progress_when_unaffordable() {
        // No income and no storage means we cannot finish any work.
        let economy = test_economy(0.0, 0.0, 0.0, 0.0);
        let cost = BuildTargetStats {
            build_cost_mass: 100.0,
            build_cost_energy: 100.0,
            build_time: 100.0,
        };
        let t = economy.estimate_remaining_time(cost, 10.0);
        assert!(t.is_infinite(), "expected infinity, got {t}");
    }

    #[test]
    fn estimate_zero_work_is_instant() {
        let economy = test_economy(0.0, 0.0, 0.0, 0.0);
        let cost = BuildTargetStats {
            build_cost_mass: 100.0,
            build_cost_energy: 100.0,
            build_time: 0.0,
        };
        let t = economy.estimate_remaining_time(cost, 10.0);
        assert!((t - 0.0).abs() < 1e-9, "expected 0s, got {t}");
    }
}
