//! The `schedule` command: compute a build order for an eco or unit target.

use std::fs;
use std::path::Path;

use faf_build_scheduler::{
    EcoScheduleInput, EcoScheduleRequest, EcoTarget, Scheduler, UnitScheduleInput,
    UnitScheduleRequest,
};
use faf_sim::units::{TechLevel, UnitId, UnitKind};

use crate::command_line::{ScheduleEcoArgs, ScheduleMode, ScheduleUnitArgs};

/// Entry point for the `schedule` command.
pub fn run(mode: ScheduleMode) {
    match mode {
        ScheduleMode::Eco(args) => run_eco(args),
        ScheduleMode::Unit(args) => run_unit(args),
    }
}

fn run_eco(args: ScheduleEcoArgs) {
    let scheduler = Scheduler::from_default_units(args.algorithm).unwrap_or_else(|e| {
        eprintln!("Failed to load blueprint library: {e}");
        std::process::exit(1);
    });

    let input: EcoScheduleInput = read_json(&args.input).unwrap_or_else(|e| {
        eprintln!("Failed to read input file: {e}");
        std::process::exit(1);
    });

    let inventory = parse_inventory(&input.initial_inventory);
    let target = EcoTarget {
        mass_production: input.target_mass_production,
        energy_production: input.target_energy_production,
        mass_storage_cap: None,
        energy_storage_cap: None,
        tolerance: input.tolerance,
    };

    if target.mass_production.is_none() && target.energy_production.is_none() {
        eprintln!("At least one of target_mass_production or target_energy_production must be set");
        std::process::exit(1);
    }

    let request = EcoScheduleRequest {
        initial_eco: input.initial_eco,
        initial_inventory: inventory,
        target,
        options: input.options,
    };

    let schedule = scheduler.schedule_eco(&request).unwrap_or_else(|e| {
        eprintln!("Scheduling failed: {e}");
        std::process::exit(1);
    });

    write_output(&args.output, &schedule.plan);
    eprintln!(
        "Scheduled eco target in {:.2}s ({} steps). Output written to {}",
        schedule.total_time_seconds,
        schedule.steps.len(),
        args.output.display()
    );
}

fn run_unit(args: ScheduleUnitArgs) {
    let scheduler = Scheduler::from_default_units(args.algorithm).unwrap_or_else(|e| {
        eprintln!("Failed to load blueprint library: {e}");
        std::process::exit(1);
    });

    let input: UnitScheduleInput = read_json(&args.input).unwrap_or_else(|e| {
        eprintln!("Failed to read input file: {e}");
        std::process::exit(1);
    });

    let inventory = parse_inventory(&input.initial_inventory);
    let target = parse_unit_kind(&input.target).unwrap_or_else(|e| {
        eprintln!("Invalid target unit '{}': {e}", input.target);
        std::process::exit(1);
    });

    let request = UnitScheduleRequest {
        initial_eco: input.initial_eco,
        initial_inventory: inventory,
        target,
        options: input.options,
    };

    let schedule = scheduler.schedule_unit(&request).unwrap_or_else(|e| {
        eprintln!("Scheduling failed: {e}");
        std::process::exit(1);
    });

    write_output(&args.output, &schedule.plan);
    eprintln!(
        "Scheduled unit target in {:.2}s ({} steps). Output written to {}",
        schedule.total_time_seconds,
        schedule.steps.len(),
        args.output.display()
    );
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let text = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

fn parse_inventory(strings: &[String]) -> Vec<UnitKind> {
    strings
        .iter()
        .map(|s| {
            parse_unit_kind(s).unwrap_or_else(|e| {
                eprintln!("Invalid inventory entry '{s}': {e}");
                std::process::exit(1);
            })
        })
        .collect()
}

fn write_output<T: serde::Serialize>(path: &Path, value: &T) {
    let json = serde_json::to_string_pretty(value).unwrap_or_else(|e| {
        eprintln!("Failed to serialize output: {e}");
        std::process::exit(1);
    });
    fs::write(path, json).unwrap_or_else(|e| {
        eprintln!("Failed to write output {}: {e}", path.display());
        std::process::exit(1);
    });
}

fn parse_unit_kind(s: &str) -> Result<UnitKind, String> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("Commander") {
        return Ok(UnitKind::Commander);
    }
    if s.eq_ignore_ascii_case("CapT2Mex") {
        return Ok(UnitKind::CapT2Mex);
    }
    if s.eq_ignore_ascii_case("CapT3Mex") {
        return Ok(UnitKind::CapT3Mex);
    }
    if s.eq_ignore_ascii_case("EnergyStorage") {
        return Ok(UnitKind::EnergyStorage);
    }
    if s.eq_ignore_ascii_case("Experimental") {
        return Ok(UnitKind::Experimental);
    }

    if let Some(inner) = s.strip_prefix("Engineer(") {
        if let Some(rest) = inner.strip_suffix(")") {
            return Ok(UnitKind::Engineer(parse_tech(rest)?));
        }
    }
    if let Some(inner) = s.strip_prefix("Factory(") {
        if let Some(rest) = inner.strip_suffix(")") {
            return Ok(UnitKind::Factory(parse_tech(rest)?));
        }
    }
    if let Some(inner) = s.strip_prefix("Mex(") {
        if let Some(rest) = inner.strip_suffix(")") {
            return Ok(UnitKind::Mex(parse_tech(rest)?));
        }
    }
    if let Some(inner) = s.strip_prefix("Pgen(") {
        if let Some(rest) = inner.strip_suffix(")") {
            return Ok(UnitKind::Pgen(parse_tech(rest)?));
        }
    }

    Ok(UnitKind::Unique(UnitId(s.to_string())))
}

fn parse_tech(s: &str) -> Result<TechLevel, String> {
    match s.trim() {
        "T1" => Ok(TechLevel::T1),
        "T2" => Ok(TechLevel::T2),
        "T3" => Ok(TechLevel::T3),
        "T4" => Ok(TechLevel::T4),
        other => Err(format!("unknown tech level '{other}'")),
    }
}
