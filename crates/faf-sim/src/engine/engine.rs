//! Deterministic tick-based eco engine.
//!
//! `EcoEngine` owns the authoritative economy state and the simulation clock.
//! It is intentionally synchronous and deterministic: given the same starting
//! economy, advancing by the same number of ticks always produces the same
//! economy state.
//!
//! The engine's internal timeline is measured in integer [`GameTick`]s, but the
//! public API accepts seconds. The engine converts between the two using a fixed
//! `ticks_per_second` rate. This lets callers think in human time while the
//! simulation stays lockstep-deterministic.
//!
//! `EcoEngine` is deliberately unit-agnostic. It knows about mass, energy, build
//! power, and storage, but it does not know how units produce income or how
//! build orders are represented. Unit/build-order state lives in
//! [`UnitGraph`](crate::engine::unit_graph::UnitGraph); a higher-level
//! simulation layer coordinates the two.

use crate::economy::{apply_tick_graph, compute_drain, EconomyState, RequestedBuildPower};
use crate::quantities::{Energy, Mass};
use faf_units::BuildTargetStats;

use super::tick::GameTick;

/// Deterministic tick-based engine for economy state.
#[derive(Debug, Clone)]
pub struct EcoEngine {
    /// Current position on the simulation timeline.
    pub tick: GameTick,
    /// Number of simulation ticks per real-time second.
    ///
    /// This determines the granularity of the simulation. Higher values make
    /// the simulation more fine-grained but require more ticks to cover the
    /// same wall-clock duration.
    pub ticks_per_second: u64,
    /// Number of ticks between issuing a command and executing it.
    ///
    /// A delay of 0 means commands execute on the tick they are issued. A
    /// non-zero delay mimics the command lag used by RTS multiplayer: the agent
    /// observes state at tick `T`, issues a command, and the command executes at
    /// tick `T + command_delay_ticks`.
    pub command_delay_ticks: u64,
    /// Authoritative economy state.
    pub economy: EconomyState,
}

impl EcoEngine {
    /// Create a new engine starting from the given economy state.
    ///
    /// The engine starts at tick 0 with no command delay.
    pub fn new(economy: EconomyState, ticks_per_second: u64) -> Self {
        Self::with_delay(economy, ticks_per_second, 0.0)
    }

    /// Create a new engine with a specific command delay.
    ///
    /// `command_delay_seconds` is the wall-clock time between issuing a command
    /// and its execution. It is converted to integer ticks using
    /// `ticks_per_second`. Use `0.0` for immediate execution (single-player
    /// training) and a small positive value (e.g., `0.2` for 200 ms) for
    /// multiplayer-style lag.
    pub fn with_delay(
        economy: EconomyState,
        ticks_per_second: u64,
        command_delay_seconds: f64,
    ) -> Self {
        let command_delay_ticks = Self::seconds_to_ticks(ticks_per_second, command_delay_seconds);
        Self {
            tick: GameTick::FIRST,
            ticks_per_second,
            command_delay_ticks,
            economy,
        }
    }

    /// Duration of a single tick in seconds.
    fn dt(&self) -> f64 {
        1.0 / self.ticks_per_second as f64
    }

    /// Convert a wall-clock duration to simulation ticks.
    pub fn seconds_to_ticks(ticks_per_second: u64, seconds: f64) -> u64 {
        (seconds * ticks_per_second as f64).round() as u64
    }

    /// Convert simulation ticks to a wall-clock duration.
    pub fn ticks_to_seconds(ticks_per_second: u64, ticks: u64) -> f64 {
        ticks as f64 / ticks_per_second as f64
    }

    /// The wall-clock time represented by the current tick.
    pub fn time_seconds(&self) -> f64 {
        self.tick.0 as f64 / self.ticks_per_second as f64
    }

    /// Return the tick on which a command issued now would execute.
    pub fn schedule_now(&self) -> GameTick {
        self.tick
    }

    /// Return the tick on which a command issued now would execute after the
    /// engine's configured command delay.
    pub fn schedule(&self) -> GameTick {
        self.tick.advance(self.command_delay_ticks)
    }

    /// Return the tick on which a command issued now would execute after a
    /// specific wall-clock delay.
    pub fn schedule_in(&self, delay_seconds: f64) -> GameTick {
        let delay_ticks = Self::seconds_to_ticks(self.ticks_per_second, delay_seconds);
        self.tick.advance(delay_ticks)
    }

    /// Return the tick on which a command issued now would execute after a
    /// specific delay in ticks.
    pub fn schedule_with_delay(&self, delay_ticks: u64) -> GameTick {
        self.tick.advance(delay_ticks)
    }

    /// Advance the simulation by one tick.
    ///
    /// Applies idle income to the economy and increments the tick counter.
    /// Active construction drains are the responsibility of the caller (usually
    /// [`UnitGraph::tick`](crate::engine::unit_graph::UnitGraph::tick)).
    pub fn step(&mut self) {
        self.apply_idle_income(self.dt());
        self.tick = self.tick.next();
    }

    /// Run the simulation for a fixed number of ticks, applying idle income each
    /// tick.
    pub fn run_for(&mut self, ticks: u64) {
        let end_tick = self.tick.advance(ticks);
        while self.tick < end_tick {
            self.step();
        }
    }

    /// Run the simulation for a fixed wall-clock duration.
    ///
    /// The duration is converted to ticks using the engine's `ticks_per_second`.
    pub fn run_for_seconds(&mut self, seconds: f64) {
        let ticks = Self::seconds_to_ticks(self.ticks_per_second, seconds);
        self.run_for(ticks);
    }

    /// Project the economy forward `seconds` without mutating the engine.
    ///
    /// The projection clones the current economy, applies idle income for the
    /// requested number of ticks, and returns the final economy. The original
    /// engine is unchanged.
    pub fn simulate_eco_for(&self, seconds: f64) -> EcoForecast {
        let mut economy = self.economy;
        let ticks = Self::seconds_to_ticks(self.ticks_per_second, seconds);
        let dt = self.dt();
        for _ in 0..ticks {
            Self::apply_idle_income_to(&mut economy, dt);
        }
        EcoForecast {
            final_economy: economy,
            final_time: self.time_seconds() + ticks as f64 * dt,
        }
    }

    /// Compute the effective mass and energy drained when applying `build_power`
    /// to a target with `cost` for `seconds`.
    ///
    /// This is a pure economy calculation: it starts from the engine's current
    /// economy, assumes income rates and build power stay constant, and ticks
    /// forward using the same stall logic as the full simulation. If the target
    /// would finish before `seconds` elapse, the drain stops at completion.
    ///
    /// Returns `None` if the target has zero build time (instant build).
    pub fn effective_drain(
        &self,
        seconds: f64,
        build_power: f64,
        cost: &BuildTargetStats,
    ) -> Option<(Mass, Energy)> {
        if seconds <= 0.0 || build_power <= 0.0 {
            return Some((Mass::zero(), Energy::zero()));
        }

        let drain = compute_drain(cost, RequestedBuildPower(build_power))?;
        let mut economy = self.economy;
        let mut total_mass = Mass::zero();
        let mut total_energy = Energy::zero();
        let mut remaining_work = cost.build_time;
        let dt = self.dt();
        let ticks = Self::seconds_to_ticks(self.ticks_per_second, seconds);

        for _ in 0..ticks {
            if remaining_work <= 0.0 {
                break;
            }

            let result =
                apply_tick_graph(drain.mass_per_second, drain.energy_per_second, &economy, dt);

            let progress = result.effective_factor * build_power * dt;
            if progress <= 0.0 {
                economy.mass_storage = result.new_mass_storage;
                economy.energy_storage = result.new_energy_storage;
                continue;
            }

            let actual_work = progress.min(remaining_work);
            let fraction = actual_work / progress;

            total_mass = total_mass + result.mass_consumed * fraction;
            total_energy = total_energy + result.energy_consumed * fraction;
            remaining_work -= actual_work;

            economy.mass_storage = result.new_mass_storage;
            economy.energy_storage = result.new_energy_storage;
        }

        Some((total_mass, total_energy))
    }

    fn apply_idle_income(&mut self, dt: f64) {
        Self::apply_idle_income_to(&mut self.economy, dt);
    }

    fn apply_idle_income_to(economy: &mut EconomyState, dt: f64) {
        use crate::quantities::Time;
        let dt = Time::from_raw(dt);
        economy.mass_storage = (economy.mass_storage + economy.net_mass_income * dt)
            .min(economy.mass_storage_cap)
            .max(Mass::zero());
        economy.energy_storage = (economy.energy_storage + economy.net_energy_income * dt)
            .min(economy.energy_storage_cap)
            .max(Energy::zero());
    }
}

/// Result of a pure eco forecast from [`EcoEngine::simulate_eco_for`].
#[derive(Debug, Clone)]
pub struct EcoForecast {
    /// Economy state at the end of the forecast window.
    pub final_economy: EconomyState,
    /// Simulation time at the end of the forecast window.
    pub final_time: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quantities::{EnergyRate, MassRate};

    fn default_economy() -> EconomyState {
        EconomyState {
            net_mass_income: MassRate::from_raw(1.0),
            net_energy_income: EnergyRate::from_raw(20.0),
            mass_storage: Mass::from_raw(650.0),
            energy_storage: Energy::from_raw(3900.0),
            mass_storage_cap: Mass::from_raw(650.0),
            energy_storage_cap: Energy::from_raw(3900.0),
        }
    }

    #[test]
    fn engine_starts_at_tick_zero() {
        let engine = EcoEngine::new(default_economy(), 1);
        assert_eq!(engine.tick, GameTick::FIRST);
        assert_eq!(engine.ticks_per_second, 1);
        assert_eq!(engine.command_delay_ticks, 0);
    }

    #[test]
    fn empty_step_advances_tick() {
        let mut engine = EcoEngine::new(default_economy(), 10);
        engine.step();
        assert_eq!(engine.tick, GameTick(1));
    }

    #[test]
    fn command_delay_delays_execution_tick() {
        let engine = EcoEngine::with_delay(default_economy(), 1, 3.0);
        assert_eq!(engine.command_delay_ticks, 3);
        assert_eq!(engine.schedule(), GameTick(3));
    }

    #[test]
    fn schedule_in_uses_seconds() {
        let engine = EcoEngine::with_delay(default_economy(), 10, 0.0);
        assert_eq!(engine.schedule_in(0.5), GameTick(5));
    }

    #[test]
    fn time_seconds_tracks_wall_clock() {
        let mut engine = EcoEngine::new(default_economy(), 10);
        assert!((engine.time_seconds() - 0.0).abs() < f64::EPSILON);

        engine.run_for(5);
        assert!((engine.time_seconds() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn forecast_idle_income_clamps_to_cap() {
        let engine = EcoEngine::new(default_economy(), 1);

        let initial_mass_cap = engine.economy.mass_storage_cap.value();
        let initial_energy_cap = engine.economy.energy_storage_cap.value();

        let forecast = engine.simulate_eco_for(10.0);

        assert!((forecast.final_time - 10.0).abs() < 1e-9);
        assert!((forecast.final_economy.mass_storage.value() - initial_mass_cap).abs() < 1e-9);
        assert!((forecast.final_economy.energy_storage.value() - initial_energy_cap).abs() < 1e-9);
    }

    #[test]
    fn forecast_does_not_mutate_engine() {
        let engine = EcoEngine::new(default_economy(), 1);

        let initial_tick = engine.tick;
        let initial_time = engine.time_seconds();
        let initial_mass = engine.economy.mass_storage.value();

        let _forecast = engine.simulate_eco_for(10.0);

        assert_eq!(engine.tick, initial_tick);
        assert!((engine.time_seconds() - initial_time).abs() < f64::EPSILON);
        assert!((engine.economy.mass_storage.value() - initial_mass).abs() < f64::EPSILON);
    }

    #[test]
    fn effective_drain_equals_cost_when_unstalled() {
        let engine = EcoEngine::new(default_economy(), 10);

        let cost = faf_units::BuildTargetStats {
            build_cost_mass: 100.0,
            build_cost_energy: 500.0,
            build_time: 10.0,
        };

        let (mass, energy) = engine.effective_drain(10.0, 10.0, &cost).unwrap();

        assert!((mass.value() - 100.0).abs() < 1e-9);
        assert!((energy.value() - 500.0).abs() < 1e-9);
    }

    #[test]
    fn effective_drain_stops_at_completion() {
        let engine = EcoEngine::new(default_economy(), 10);

        let cost = faf_units::BuildTargetStats {
            build_cost_mass: 100.0,
            build_cost_energy: 500.0,
            build_time: 10.0,
        };

        let (mass, energy) = engine.effective_drain(10.0, 20.0, &cost).unwrap();

        assert!((mass.value() - 100.0).abs() < 1e-9);
        assert!((energy.value() - 500.0).abs() < 1e-9);
    }

    #[test]
    fn effective_drain_is_reduced_when_stalled() {
        let mut economy = default_economy();
        economy.energy_storage = Energy::zero();
        economy.energy_storage_cap = Energy::zero();
        economy.net_energy_income = EnergyRate::zero();
        let engine = EcoEngine::new(economy, 10);

        let cost = faf_units::BuildTargetStats {
            build_cost_mass: 100.0,
            build_cost_energy: 500.0,
            build_time: 10.0,
        };

        let (mass, energy) = engine.effective_drain(10.0, 10.0, &cost).unwrap();

        assert!(mass.value() < 1e-9);
        assert!(energy.value() < 1e-9);
    }
}
