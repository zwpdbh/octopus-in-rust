//! The `schedule` command: compute a build order for an eco or unit target.

use std::fs;
use std::path::Path;

use faf_build_scheduler::{
    AlgorithmKind, EcoScheduleRequest, EcoTarget, Scheduler, SearchOptions, UnitScheduleRequest,
};
use faf_sim::runtime::EcoSnapshot;
use faf_sim::units::{BlueprintLibrary, TechLevel, UnitId, UnitKind};

use crate::command_line::{ScheduleEcoArgs, ScheduleMode, ScheduleUnitArgs};

/// Entry point for the `schedule` command.
pub fn run(mode: ScheduleMode) {
    match mode {
        ScheduleMode::Eco(args) => run_eco(args),
        ScheduleMode::Unit(args) => run_unit(args),
    }
}

fn run_eco(args: ScheduleEcoArgs) {
    let (scheduler, initial_eco, inventory) =
        load_shared(&args.units_file, &args.eco, args.inventory.as_deref());

    let target: EcoTarget = read_json(&args.target).unwrap_or_else(|e| {
        eprintln!("Failed to read target file: {e}");
        std::process::exit(1);
    });

    let request = EcoScheduleRequest {
        initial_eco,
        initial_inventory: inventory,
        target,
        options: SearchOptions {
            max_search_seconds: args.max_search_seconds,
            simulation_max_time_seconds: args.simulation_max_time_seconds,
        },
    };

    let schedule = scheduler.schedule_eco(&request).unwrap_or_else(|e| {
        eprintln!("Scheduling failed: {e}");
        std::process::exit(1);
    });

    write_output(&args.output, &schedule.build_queue);
    eprintln!(
        "Scheduled eco target in {:.2}s ({} steps). Output written to {}",
        schedule.total_time_seconds,
        schedule.steps.len(),
        args.output.display()
    );
}

fn run_unit(args: ScheduleUnitArgs) {
    let (scheduler, initial_eco, inventory) =
        load_shared(&args.units_file, &args.eco, args.inventory.as_deref());

    let target = parse_unit_kind(&args.target).unwrap_or_else(|e| {
        eprintln!("Invalid target unit: {e}");
        std::process::exit(1);
    });

    let request = UnitScheduleRequest {
        initial_eco,
        initial_inventory: inventory,
        target,
        options: SearchOptions {
            max_search_seconds: args.max_search_seconds,
            simulation_max_time_seconds: args.simulation_max_time_seconds,
        },
    };

    let schedule = scheduler.schedule_unit(&request).unwrap_or_else(|e| {
        eprintln!("Scheduling failed: {e}");
        std::process::exit(1);
    });

    write_output(&args.output, &schedule.build_queue);
    eprintln!(
        "Scheduled unit target in {:.2}s ({} steps). Output written to {}",
        schedule.total_time_seconds,
        schedule.steps.len(),
        args.output.display()
    );
}

fn load_shared(
    units_file: &Path,
    eco_path: &Path,
    inventory_path: Option<&Path>,
) -> (Scheduler, EcoSnapshot, Vec<UnitKind>) {
    let index = load_units(units_file);
    let library = BlueprintLibrary::new(index);
    let scheduler = Scheduler::with_algorithm(library, AlgorithmKind::Placeholder);

    let initial_eco: EcoSnapshot = read_json(eco_path).unwrap_or_else(|e| {
        eprintln!("Failed to read economy file: {e}");
        std::process::exit(1);
    });

    let inventory: Vec<UnitKind> = if let Some(path) = inventory_path {
        let strings: Vec<String> = read_json(path).unwrap_or_else(|e| {
            eprintln!("Failed to read inventory file: {e}");
            std::process::exit(1);
        });
        strings
            .into_iter()
            .map(|s| {
                parse_unit_kind(&s).unwrap_or_else(|e| {
                    eprintln!("Invalid inventory entry '{s}': {e}");
                    std::process::exit(1);
                })
            })
            .collect()
    } else {
        vec![UnitKind::Commander]
    };

    (scheduler, initial_eco, inventory)
}

fn load_units(path: &Path) -> faf_units::DataIndex {
    let text = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("Failed to read units file {}: {e}", path.display());
        std::process::exit(1);
    });
    serde_json::from_str::<faf_units::DataIndex>(&text).unwrap_or_else(|e| {
        eprintln!("Failed to parse units file {}: {e}", path.display());
        std::process::exit(1);
    })
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let text = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

fn write_output(path: &Path, queue: &faf_sim::runtime::BuildQueue) {
    let json = serde_json::to_string_pretty(queue).unwrap_or_else(|e| {
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
