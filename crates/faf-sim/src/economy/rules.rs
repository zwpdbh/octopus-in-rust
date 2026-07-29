//! Pure numerical rules for FAF build-order simulation.
//!
//! Supreme Commander / FAF uses a continuous-drain build model:
//!
//! - A project consumes mass and energy every second while it is being built.
//! - The consumption rate scales with the build power assigned to the project.
//! - If available income cannot cover the drain, the project stalls and its
//!   effective build power drops.
//!
//! This module provides the pure math for computing drain rates and stall
//! factors for a single project. The global/graph tick that combines multiple
//! projects lives in [`super::tick`].

use crate::{
    quantities::{Energy, EnergyRate, Mass, MassRate, Storage, Time},
    units::{BlueprintLibrary, UnitCost, UnitKind},
};
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
pub fn total_build_power(library: &BlueprintLibrary, builders: &[UnitKind]) -> RequestedBuildPower {
    RequestedBuildPower(
        builders
            .iter()
            .map(|kind| library.build_power(kind))
            .filter(|&power| power > 0.0)
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
    pub mass_per_second: MassRate,
    pub energy_per_second: EnergyRate,
}

impl EcoFlow {
    /// Zero flow.
    pub const ZERO: Self = Self {
        mass_per_second: MassRate::from_raw(0.0),
        energy_per_second: EnergyRate::from_raw(0.0),
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
                mass_per_second: MassRate::from_raw(d.mass_per_second),
                energy_per_second: EnergyRate::from_raw(d.energy_per_second),
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
#[derive(Debug, Clone)]
pub struct ResourceProducer<'a> {
    library: &'a BlueprintLibrary,
    kind: UnitKind,
    cost: UnitCost,
}

impl<'a> ResourceProducer<'a> {
    /// Create a producer view for `kind`.
    ///
    /// Returns `None` if the unit has no definition, zero mass cost, or no
    /// resource production.
    pub fn new(library: &'a BlueprintLibrary, kind: &'a UnitKind) -> Option<Self> {
        let cost = library.unit_build_cost(kind)?;
        if cost.mass <= 0.0 {
            return None;
        }
        let mass_prod = library.production_per_second_mass(kind);
        let energy_prod = library.production_per_second_energy(kind);
        if mass_prod <= 0.0 && energy_prod <= 0.0 {
            return None;
        }
        Some(Self {
            library,
            kind: kind.clone(),
            cost,
        })
    }

    /// Build stats of this producer.
    pub fn stats(&self) -> UnitCost {
        self.cost
    }

    /// Gross production flow.
    pub fn production(&self) -> EcoFlow {
        EcoFlow {
            mass_per_second: MassRate::from_raw(
                self.library.production_per_second_mass(&self.kind),
            ),
            energy_per_second: EnergyRate::from_raw(
                self.library.production_per_second_energy(&self.kind),
            ),
        }
    }

    /// Maintenance consumption flow.
    pub fn maintenance(&self) -> EcoFlow {
        EcoFlow {
            mass_per_second: MassRate::zero(),
            energy_per_second: EnergyRate::from_raw(
                self.library
                    .maintenance_consumption_per_second_energy(&self.kind),
            ),
        }
    }

    /// Net production after maintenance.
    pub fn net_production(&self) -> EcoFlow {
        self.production() - self.maintenance()
    }

    /// FAF `ProductionPerSecondMass` per mass invested.
    pub fn mass_efficiency(&self) -> f64 {
        self.production().mass_per_second.value() / self.cost.mass
    }

    /// FAF `ProductionPerSecondEnergy` per mass invested.
    pub fn energy_efficiency(&self) -> f64 {
        self.production().energy_per_second.value() / self.cost.mass
    }
}

/// Summarize the net mass and energy flow for a collection of owned units and
/// active construction projects.
///
/// The returned flow can be negative when consumption exceeds production, just
/// like the in-game economy display during heavy construction.
pub fn summarize_economy(
    library: &BlueprintLibrary,
    owned: &[UnitKind],
    active_projects: &[&BuildProject],
) -> EcoFlow {
    let production: EcoFlow = owned
        .iter()
        .map(|kind| EcoFlow {
            mass_per_second: MassRate::from_raw(library.production_per_second_mass(kind)),
            energy_per_second: EnergyRate::from_raw(library.production_per_second_energy(kind)),
        })
        .sum();
    let maintenance: EcoFlow = owned
        .iter()
        .map(|kind| EcoFlow {
            mass_per_second: MassRate::zero(),
            energy_per_second: EnergyRate::from_raw(
                library.maintenance_consumption_per_second_energy(kind),
            ),
        })
        .sum();
    let construction: EcoFlow = active_projects.iter().map(|p| p.consumption()).sum();
    production - maintenance - construction
}

/// Internal, mutable economy state used to drive the simulation.
///
/// This is the simulator's working copy of an army's economy. It uses strongly
/// typed quantities (`MassRate`, `EnergyRate`, `Storage<Mass>`, etc.) and is
/// updated every tick by the runtime systems.
///
/// For the public, point-in-time record that is emitted to consumers (UI,
/// WebSocket, ML models), see [`EcoSnapshot`](crate::runtime::EcoSnapshot).
pub use faf_sim_shared::GameEcoParameters;

/// Result of applying a drain to an economy state for one second.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TickResult {
    /// Effective build power after considering stalls.
    pub effective_build_power: EffectiveBuildPower,
    /// Mass actually consumed.
    pub mass_consumed: Mass,
    /// Energy actually consumed.
    pub energy_consumed: Energy,
    /// New mass storage after the tick.
    pub new_mass_storage: Storage<Mass>,
    /// New energy storage after the tick.
    pub new_energy_storage: Storage<Energy>,
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
pub fn apply_tick(requested: &BuildDrain, state: &GameEcoParameters, dt: f64) -> TickResult {
    let dt = Time::from_raw(dt);

    // Gross income during this tick, ignoring the drain.
    let production_per_second_mass = state.production_per_second_mass * dt;
    let production_per_second_energy = state.production_per_second_energy * dt;
    let maintenance_energy = state.maintenance_consumption_per_second_energy * dt;

    // Requested consumption over the tick.
    let requested_mass = Mass::from_raw(requested.mass_per_second * dt.value());
    let requested_energy = Energy::from_raw(requested.energy_per_second * dt.value());

    // Maximum sustainable fraction of requested drain before each resource
    // would hit zero. If income already covers drain, the factor is 1.0.
    // Energy must pay maintenance before it can be spent on construction.
    let mass_factor = if requested_mass.value() <= 0.0 {
        1.0
    } else {
        let available = (state.mass_storage.current + production_per_second_mass).max(Mass::zero());
        (available / requested_mass).min(1.0)
    };

    let energy_factor = if requested_energy.value() <= 0.0 {
        1.0
    } else {
        let available = (state.energy_storage.current + production_per_second_energy
            - maintenance_energy)
            .max(Energy::zero());
        (available / requested_energy).min(1.0)
    };

    // The project can only run as fast as the most constrained resource.
    let effective_factor = mass_factor.min(energy_factor);
    let effective_build_power = requested
        .assigned_build_power
        .to_effective(effective_factor);

    let mass_consumed = requested_mass * effective_factor;
    let energy_consumed = requested_energy * effective_factor;

    let new_mass_current = (state.mass_storage.current + production_per_second_mass
        - mass_consumed)
        .min(state.mass_storage.cap)
        .max(Mass::zero());
    let new_energy_current = (state.energy_storage.current + production_per_second_energy
        - maintenance_energy
        - energy_consumed)
        .min(state.energy_storage.cap)
        .max(Energy::zero());
    let new_mass_storage = Storage::new(new_mass_current, state.mass_storage.cap);
    let new_energy_storage = Storage::new(new_energy_current, state.energy_storage.cap);

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
    pub fn new(target: UnitKind, library: &BlueprintLibrary) -> Option<Self> {
        let cost = library.unit_build_cost(&target)?;
        if cost.build_time <= 0.0 {
            return None;
        }
        Some(Self {
            target_kind: target,
            target: cost,
            assigned_build_power: RequestedBuildPower(0.0),
            remaining_work: cost.build_time,
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
    pub fn tick(&mut self, state: &mut GameEcoParameters, dt: f64) -> TickOutcome {
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
    use crate::units::{BlueprintLibrary, TechLevel, UnitId};

    fn load_library() -> BlueprintLibrary {
        let json = include_str!("../../../../plugins/faf-units/data/faf_units.json");
        BlueprintLibrary::new(serde_json::from_str(json).expect("embedded index should parse"))
    }

    #[test]
    fn monkeylord_drain_at_base_build_power() {
        let units = load_library();
        let monkeylord = UnitKind::Unique(UnitId("URL0402".to_string()));
        let cost = units
            .unit_build_cost(&monkeylord)
            .expect("Monkeylord exists");

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
        let units = load_library();
        let monkeylord = UnitKind::Unique(UnitId("URL0402".to_string()));
        let t3_eng = UnitKind::Engineer(TechLevel::T3);
        let build_power = RequestedBuildPower(units.build_power(&t3_eng));

        let drain = compute_drain(
            &units
                .unit_build_cost(&monkeylord)
                .unwrap()
                .to_target_stats(),
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
        let units = load_library();
        let acu = UnitKind::Commander;
        let t1_eng = UnitKind::Engineer(TechLevel::T1);
        let t3_eng = UnitKind::Engineer(TechLevel::T3);

        let total = total_build_power(&units, &[acu.clone(), t1_eng.clone(), t3_eng.clone()]);

        let expected =
            units.build_power(&acu) + units.build_power(&t1_eng) + units.build_power(&t3_eng);

        assert!((total.0 - expected).abs() < 1e-9);
    }

    #[test]
    fn tick_with_plenty_resources_runs_full_power() {
        let units = load_library();
        let monkeylord = UnitKind::Unique(UnitId("URL0402".to_string()));
        let drain = compute_drain(
            &units
                .unit_build_cost(&monkeylord)
                .unwrap()
                .to_target_stats(),
            RequestedBuildPower(10.0),
        )
        .expect("valid drain");

        let state = GameEcoParameters {
            production_per_second_mass: crate::quantities::MassRate::from_raw(1000.0),
            production_per_second_energy: crate::quantities::EnergyRate::from_raw(10000.0),
            mass_storage: Storage::new(
                crate::quantities::Mass::from_raw(50000.0),
                crate::quantities::Mass::from_raw(100000.0),
            ),
            energy_storage: Storage::new(
                crate::quantities::Energy::from_raw(500000.0),
                crate::quantities::Energy::from_raw(1000000.0),
            ),
            ..Default::default()
        };

        let result = apply_tick(&drain, &state, 1.0);
        assert!((result.effective_build_power.0 - 10.0).abs() < 1e-9);
        assert!(!result.energy_stalled);
        assert!(!result.mass_stalled);
    }

    #[test]
    fn tick_stalls_when_energy_insufficient() {
        let units = load_library();
        let monkeylord = UnitKind::Unique(UnitId("URL0402".to_string()));
        let drain = compute_drain(
            &units
                .unit_build_cost(&monkeylord)
                .unwrap()
                .to_target_stats(),
            RequestedBuildPower(10.0),
        )
        .expect("valid drain");

        // Very little `ProductionPerSecondEnergy` and storage, plenty of mass.
        let state = GameEcoParameters {
            production_per_second_mass: crate::quantities::MassRate::from_raw(1000.0),
            production_per_second_energy: crate::quantities::EnergyRate::from_raw(0.0),
            mass_storage: Storage::new(
                crate::quantities::Mass::from_raw(50000.0),
                crate::quantities::Mass::from_raw(100000.0),
            ),
            energy_storage: Storage::new(
                crate::quantities::Energy::from_raw(drain.energy_per_second * 0.5),
                crate::quantities::Energy::from_raw(1000000.0),
            ),
            ..Default::default()
        };

        let result = apply_tick(&drain, &state, 1.0);
        assert!(result.effective_build_power.0 < 10.0);
        assert!(result.energy_stalled);
        assert!(!result.mass_stalled);
        assert!(result.new_energy_storage.current.abs() < 1e-6);
    }

    #[test]
    fn tick_stalls_when_mass_insufficient() {
        let units = load_library();
        let monkeylord = UnitKind::Unique(UnitId("URL0402".to_string()));
        let drain = compute_drain(
            &units
                .unit_build_cost(&monkeylord)
                .unwrap()
                .to_target_stats(),
            RequestedBuildPower(10.0),
        )
        .expect("valid drain");

        // Very little `ProductionPerSecondMass` and storage, plenty of energy.
        let state = GameEcoParameters {
            production_per_second_mass: crate::quantities::MassRate::from_raw(0.0),
            production_per_second_energy: crate::quantities::EnergyRate::from_raw(1000.0),
            mass_storage: Storage::new(
                crate::quantities::Mass::from_raw(drain.mass_per_second * 0.5),
                crate::quantities::Mass::from_raw(100000.0),
            ),
            energy_storage: Storage::new(
                crate::quantities::Energy::from_raw(500000.0),
                crate::quantities::Energy::from_raw(1000000.0),
            ),
            ..Default::default()
        };

        let result = apply_tick(&drain, &state, 1.0);
        assert!(result.effective_build_power.0 < 10.0);
        assert!(result.mass_stalled);
        assert!(!result.energy_stalled);
        assert!(result.new_mass_storage.current.abs() < 1e-6);
    }

    #[test]
    fn build_project_completes_with_constant_power() {
        let units = load_library();
        let t1_eng = UnitKind::Engineer(TechLevel::T1);
        let build_power = RequestedBuildPower(units.build_power(&t1_eng));

        // Build a T1 engineer with another T1 engineer.
        let mut project = BuildProject::new(t1_eng, &units).expect("valid unit");
        project.assigned_build_power = build_power;

        let mut state = GameEcoParameters {
            production_per_second_mass: crate::quantities::MassRate::from_raw(1000.0),
            production_per_second_energy: crate::quantities::EnergyRate::from_raw(10000.0),
            mass_storage: Storage::new(
                crate::quantities::Mass::from_raw(10000.0),
                crate::quantities::Mass::from_raw(1000000.0),
            ),
            energy_storage: Storage::new(
                crate::quantities::Energy::from_raw(100000.0),
                crate::quantities::Energy::from_raw(1000000.0),
            ),
            ..Default::default()
        };

        let build_time = project.target.build_time;
        let outcome = project.tick(&mut state, build_time);

        assert!(outcome.is_completed());
        assert!(project.is_complete());
        assert!((project.progress() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn build_project_progress_tracks_remaining_work() {
        let units = load_library();
        let t1_eng = UnitKind::Engineer(TechLevel::T1);
        let build_power = RequestedBuildPower(units.build_power(&t1_eng));

        let mut project = BuildProject::new(t1_eng, &units).expect("valid unit");
        let build_time = project.target.build_time;
        project.assigned_build_power = build_power;

        let mut state = GameEcoParameters {
            production_per_second_mass: crate::quantities::MassRate::from_raw(1000.0),
            production_per_second_energy: crate::quantities::EnergyRate::from_raw(10000.0),
            mass_storage: Storage::new(
                crate::quantities::Mass::from_raw(10000.0),
                crate::quantities::Mass::from_raw(1000000.0),
            ),
            energy_storage: Storage::new(
                crate::quantities::Energy::from_raw(100000.0),
                crate::quantities::Energy::from_raw(1000000.0),
            ),
            ..Default::default()
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
        let units = load_library();
        let t1_eng = UnitKind::Engineer(TechLevel::T1);
        let build_power = RequestedBuildPower(units.build_power(&t1_eng));

        let mut project = BuildProject::new(t1_eng, &units).expect("valid unit");
        let build_time = project.target.build_time;
        project.assigned_build_power = build_power;

        // No `ProductionPerSecondEnergy` and tiny storage: will stall.
        let mut state = GameEcoParameters {
            production_per_second_mass: crate::quantities::MassRate::from_raw(1000.0),
            production_per_second_energy: crate::quantities::EnergyRate::from_raw(0.0),
            mass_storage: Storage::new(
                crate::quantities::Mass::from_raw(10000.0),
                crate::quantities::Mass::from_raw(1000000.0),
            ),
            energy_storage: Storage::new(
                crate::quantities::Energy::from_raw(10.0),
                crate::quantities::Energy::from_raw(1000000.0),
            ),
            ..Default::default()
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
        let units = load_library();
        let t1_eng = UnitKind::Engineer(TechLevel::T1);
        let drain = compute_drain(
            &units.unit_build_cost(&t1_eng).unwrap().to_target_stats(),
            RequestedBuildPower(1.0),
        )
        .expect("valid drain");

        let mut state = GameEcoParameters {
            production_per_second_mass: crate::quantities::MassRate::from_raw(1000.0),
            production_per_second_energy: crate::quantities::EnergyRate::from_raw(10000.0),
            mass_storage: Storage::new(
                crate::quantities::Mass::from_raw(100.0),
                crate::quantities::Mass::from_raw(100.0),
            ),
            energy_storage: Storage::new(
                crate::quantities::Energy::from_raw(1000.0),
                crate::quantities::Energy::from_raw(1000.0),
            ),
            ..Default::default()
        };

        let result = apply_tick(&drain, &state, 1.0);

        // Storage was already at cap, so income above the drain is wasted.
        assert!(
            (result.new_mass_storage.current - crate::quantities::Mass::from_raw(100.0)).abs()
                < 1e-9
        );
        assert!(
            (result.new_energy_storage.current - crate::quantities::Energy::from_raw(1000.0)).abs()
                < 1e-9
        );
        state.mass_storage = result.new_mass_storage;
        state.energy_storage = result.new_energy_storage;

        // Tick again: since storage did not grow, nothing changes except the drain.
        let result2 = apply_tick(&drain, &state, 1.0);
        assert!(
            (result2.new_mass_storage.current - crate::quantities::Mass::from_raw(100.0)).abs()
                < 1e-9
        );
        assert!(
            (result2.new_energy_storage.current - crate::quantities::Energy::from_raw(1000.0))
                .abs()
                < 1e-9
        );
    }

    #[test]
    fn storage_buffers_during_zero_income() {
        let units = load_library();
        let t1_eng = UnitKind::Engineer(TechLevel::T1);
        let build_power = RequestedBuildPower(units.build_power(&t1_eng));

        let mut project = BuildProject::new(t1_eng, &units).expect("valid unit");
        let total_mass = project.target.mass;
        let total_energy = project.target.energy;
        let build_time = project.target.build_time;
        project.assigned_build_power = build_power;

        // No income, but enough storage to pay the full cost.
        let mut state = GameEcoParameters {
            production_per_second_mass: crate::quantities::MassRate::from_raw(0.0),
            production_per_second_energy: crate::quantities::EnergyRate::from_raw(0.0),
            mass_storage: Storage::new(
                crate::quantities::Mass::from_raw(total_mass),
                crate::quantities::Mass::from_raw(total_mass * 2.0),
            ),
            energy_storage: Storage::new(
                crate::quantities::Energy::from_raw(total_energy),
                crate::quantities::Energy::from_raw(total_energy * 2.0),
            ),
            ..Default::default()
        };

        let outcome = project.tick(&mut state, build_time);

        assert!(outcome.is_completed());
        assert!(state.mass_storage.current.abs() < 1e-6);
        assert!(state.energy_storage.current.abs() < 1e-6);
    }

    #[test]
    fn summarize_economy_computes_net_flow() {
        let units = load_library();

        // ACU alone: produces 1 mass/s and 20 energy/s, no maintenance.
        let net = summarize_economy(&units, &[UnitKind::Commander], &[]);
        assert!((net.mass_per_second - crate::quantities::MassRate::from_raw(1.0)).abs() < 1e-9);
        assert!(
            (net.energy_per_second - crate::quantities::EnergyRate::from_raw(20.0)).abs() < 1e-9
        );

        // ACU + T1 mex: mex adds 2 mass/s but consumes 2 energy/s maintenance.
        let net = summarize_economy(
            &units,
            &[UnitKind::Commander, UnitKind::Mex(TechLevel::T1)],
            &[],
        );
        assert!((net.mass_per_second - crate::quantities::MassRate::from_raw(3.0)).abs() < 1e-9);
        assert!(
            (net.energy_per_second - crate::quantities::EnergyRate::from_raw(18.0)).abs() < 1e-9
        );

        // ACU building Monkeylord with all its BP: construction drain can make
        // net energy strongly negative.
        let mut project =
            BuildProject::new(UnitKind::Unique(UnitId("URL0402".to_string())), &units)
                .expect("valid target");
        project.assigned_build_power = RequestedBuildPower(units.build_power(&UnitKind::Commander));
        let net = summarize_economy(&units, &[UnitKind::Commander], &[&project]);
        assert!(
            net.energy_per_second < 0.0,
            "building Monkeylord should make net energy negative"
        );
    }

    #[test]
    fn resource_producers_share_uniform_metrics() {
        let units = load_library();
        let mut checked = 0;

        for kind in units.all_kinds() {
            let is_mex = matches!(kind, UnitKind::Mex(_));
            let is_pgen = matches!(kind, UnitKind::Pgen(_));
            if !is_mex && !is_pgen {
                continue;
            }

            // Special units such as the Aeon Paragon carry the production
            // categories but have no fixed production values. Only enforce the
            // common metric shape on units that actually produce something.
            let mass_prod = units.production_per_second_mass(&kind);
            let energy_prod = units.production_per_second_energy(&kind);
            let produces_mass = mass_prod > 0.0;
            let produces_energy = energy_prod > 0.0;
            if !produces_mass && !produces_energy {
                continue;
            }

            let cost = units.unit_build_cost(&kind).expect("producer has a cost");
            checked += 1;
            assert!(cost.mass > 0.0, "{:?} has no mass cost", kind);
            assert!(cost.energy > 0.0, "{:?} has no energy cost", kind);
            assert!(cost.build_time > 0.0, "{:?} has no build time", kind);

            if is_mex {
                assert!(produces_mass, "{:?} is a mex with no mass production", kind);
            }
            if is_pgen {
                assert!(
                    produces_energy,
                    "{:?} is a pgen with no energy production",
                    kind
                );
            }

            // All resource producers should be representable as a ResourceProducer.
            assert!(
                ResourceProducer::new(&units, &kind).is_some(),
                "{:?} should be a valid ResourceProducer",
                kind
            );
        }

        assert!(checked > 0, "no resource producers found in index");
    }
}
