//! Generate a SQLite dataset of simulated build-plan completion times.

use std::path::Path;

use anyhow::{Context, Result};
use faf_sim::quantities::{StepTime, Time};
use faf_sim::runtime::{BuildTask, EcoSnapshot, UnitEcoStats};
use faf_sim::sim::Simulation;
use rand::{Rng, RngExt};
use rusqlite::{Connection, Transaction};

use crate::data::normalize::NormalizationParams;
use crate::data::sample::{build_queue, extract_features, EcoPlanLabel, EcoPlanSample};

/// Configuration controlling dataset generation.
#[derive(Debug, Clone, Copy)]
pub struct GenerationConfig {
    /// Number of labeled samples to generate.
    pub sample_count: usize,
    /// Maximum simulation time in seconds before a plan is labeled NotPractical.
    pub time_limit_seconds: f64,
    /// Maximum number of tasks in a generated plan.
    pub max_tasks: usize,
    /// Maximum number of builders assigned to a single task.
    pub max_builders_per_task: usize,
    /// Maximum number of targets inside a single task.
    pub max_targets_per_task: usize,
}

impl Default for GenerationConfig {
    fn default() -> Self {
        Self {
            sample_count: 10_000,
            time_limit_seconds: 600.0,
            max_tasks: 5,
            max_builders_per_task: 3,
            max_targets_per_task: 5,
        }
    }
}

/// Generate a dataset and write it to `db_path`.
pub fn generate_dataset(db_path: &Path, config: GenerationConfig) -> Result<()> {
    let mut conn = Connection::open(db_path)
        .with_context(|| format!("Failed to open SQLite database at {}", db_path.display()))?;

    create_schema(&mut conn)?;

    let mut rng = rand::rng();

    let tx = conn
        .transaction()
        .context("Failed to start SQLite transaction")?;

    let mut stats = NormalizationParams::new();

    for i in 0..config.sample_count {
        let sample = generate_sample(&mut rng, config);
        let features = extract_features(&sample.initial_eco, &sample.plan);
        stats.update(&features);

        let label = simulate_label(&sample, config.time_limit_seconds);
        insert_sample(&tx, &features, &label, config.time_limit_seconds)?;

        if (i + 1) % 1000 == 0 {
            println!("Generated {} / {} samples", i + 1, config.sample_count);
        }
    }

    tx.commit().context("Failed to commit SQLite transaction")?;

    let norm_path = db_path.with_extension("norm.json");
    stats.save(&norm_path)?;

    println!(
        "Dataset complete: {} samples in {}",
        config.sample_count,
        db_path.display()
    );

    Ok(())
}

fn create_schema(conn: &mut Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS samples (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            features TEXT NOT NULL,
            target_time REAL NOT NULL,
            is_practical INTEGER NOT NULL
        )",
        [],
    )
    .context("Failed to create samples table")?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
        [],
    )
    .context("Failed to create metadata table")?;

    Ok(())
}

fn insert_sample(
    tx: &Transaction,
    features: &[f64],
    label: &EcoPlanLabel,
    time_limit_seconds: f64,
) -> Result<()> {
    let features_json = serde_json::to_string(features).context("Failed to serialize features")?;
    let target_time = label.regression_target(time_limit_seconds).exp();
    let is_practical = label.is_practical() as i32;

    tx.execute(
        "INSERT INTO samples (features, target_time, is_practical) VALUES (?1, ?2, ?3)",
        [
            &features_json,
            &target_time.to_string(),
            &is_practical.to_string(),
        ],
    )
    .context("Failed to insert sample")?;

    Ok(())
}

fn generate_sample<R: Rng>(rng: &mut R, config: GenerationConfig) -> EcoPlanSample {
    let initial_eco = random_eco_snapshot(rng);
    let task_count = rng.random_range(1..=config.max_tasks.max(1));
    let plan: Vec<BuildTask> = (0..task_count)
        .map(|id| random_build_task(rng, id as u32, config))
        .collect();

    EcoPlanSample {
        initial_eco,
        plan,
        label: EcoPlanLabel::NotPractical, // placeholder, replaced by simulation
    }
}

fn simulate_label(sample: &EcoPlanSample, time_limit_seconds: f64) -> EcoPlanLabel {
    let dt = StepTime::from_seconds(1).expect("1 second dt is valid");
    let max_time = Time::from_raw(time_limit_seconds);
    let queue = build_queue(&sample.initial_eco, sample.plan.clone());

    let mut sim = Simulation::new(queue, dt, Some(max_time));

    while !sim.is_finished() {
        sim.step();
    }

    let final_time = sim.current_time().value();
    if final_time < time_limit_seconds - dt.as_time().value() {
        EcoPlanLabel::Practical {
            time_seconds: final_time,
        }
    } else {
        EcoPlanLabel::NotPractical
    }
}

fn random_eco_snapshot<R: Rng>(rng: &mut R) -> EcoSnapshot {
    let mass_cap = rng.random_range(100.0..3000.0);
    let energy_cap = rng.random_range(1000.0..30000.0);

    EcoSnapshot {
        time: 0.0,
        production_per_second_mass: rng.random_range(0.0..200.0),
        production_per_second_energy: rng.random_range(0.0..5000.0),
        maintenance_consumption_per_second_energy: rng.random_range(0.0..500.0),
        mass_drain: 0.0,
        energy_drain: 0.0,
        total_mass_spent: 0.0,
        total_energy_spent: 0.0,
        mass_storage: rng.random_range(0.0..mass_cap),
        mass_storage_cap: mass_cap,
        energy_storage: rng.random_range(0.0..energy_cap),
        energy_storage_cap: energy_cap,
    }
}

fn random_build_task<R: Rng>(rng: &mut R, id: u32, config: GenerationConfig) -> BuildTask {
    let builder_count = rng.random_range(1..=config.max_builders_per_task.max(1));
    let target_count = rng.random_range(1..=config.max_targets_per_task.max(1));

    let builders: Vec<UnitEcoStats> = (0..builder_count).map(|_| random_builder(rng)).collect();
    let targets: Vec<UnitEcoStats> = (0..target_count).map(|_| random_target(rng)).collect();

    BuildTask {
        id,
        start_after: Time::from_raw(1.0),
        builders,
        targets,
    }
}

fn random_builder<R: Rng>(rng: &mut R) -> UnitEcoStats {
    UnitEcoStats {
        build_power: rng.random_range(1.0..50.0),
        mass_cost: 0.0,
        energy_cost: 0.0,
        build_time: 0.0,
        production_per_second_mass: 0.0,
        production_per_second_energy: 0.0,
        maintenance_consumption_per_second_energy: rng.random_range(0.0..20.0),
        mass_storage: 0.0,
        energy_storage: 0.0,
        unit_id: None,
    }
}

fn random_target<R: Rng>(rng: &mut R) -> UnitEcoStats {
    UnitEcoStats {
        build_power: 0.0,
        mass_cost: rng.random_range(1.0..20000.0),
        energy_cost: rng.random_range(1.0..100000.0),
        build_time: rng.random_range(1.0..5000.0),
        production_per_second_mass: rng.random_range(0.0..50.0),
        production_per_second_energy: rng.random_range(0.0..500.0),
        maintenance_consumption_per_second_energy: rng.random_range(0.0..50.0),
        mass_storage: rng.random_range(0.0..1000.0),
        energy_storage: rng.random_range(0.0..10000.0),
        unit_id: None,
    }
}
