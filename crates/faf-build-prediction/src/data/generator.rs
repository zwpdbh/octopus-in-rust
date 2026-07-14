//! Generate a SQLite dataset of simulated build plans and their completion times.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use faf_sim::quantities::{StepTime, Time};
use faf_sim::runtime::{BuildTask, EcoSnapshot, UnitEcoStats};
use faf_sim::sim::Simulation;
use faf_sim::units::{TechLevel, UnitDef, UnitKind, Units};
use rand::{Rng, RngExt};
use rusqlite::{Connection, Transaction};

use crate::data::normalize::NormalizationParams;
use crate::data::sample::{build_queue, extract_sequence_features, EcoPlanLabel, EcoPlanSample};

/// Configuration controlling dataset generation.
#[derive(Debug, Clone, Copy)]
pub struct GenerationConfig {
    /// Number of labeled samples to generate.
    pub sample_count: usize,
    /// Practical time limit in seconds; slower plans are labeled NotPractical.
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

/// Source of units used to sample builders and targets.
#[derive(Debug, Clone)]
enum UnitSource {
    /// Pure synthetic units drawn from fixed uniform ranges.
    Synthetic,
    /// Real FAF units loaded from `faf-units`.
    Real {
        units: Units,
        builders: Vec<UnitKind>,
        targets: Vec<UnitKind>,
    },
}

/// Builder-pattern generator for a `faf-build-prediction` training dataset.
///
/// # Example
///
/// ```rust,ignore
/// DatasetGenerator::new(GenerationConfig::default())
///     .with_units_file(Path::new("plugins/faf-units/data/faf_units.json"))?
///     .generate(Path::new("data/dataset.db"))?;
/// ```
pub struct DatasetGenerator {
    config: GenerationConfig,
    source: UnitSource,
}

impl DatasetGenerator {
    /// Create a generator that uses synthetic units.
    pub fn new(config: GenerationConfig) -> Self {
        Self {
            config,
            source: UnitSource::Synthetic,
        }
    }

    /// Configure the generator to sample from a real FAF unit database.
    pub fn with_units_file(mut self, path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read units file {}", path.display()))?;
        let index: faf_units::DataIndex = serde_json::from_str(&text)
            .with_context(|| format!("Failed to parse units file {}", path.display()))?;
        let units = Units::new(index);

        let builders: Vec<UnitKind> = units
            .defs()
            .iter()
            .filter(|(_, def)| def.build_rate() > 0.0)
            .map(|(kind, _)| kind.clone())
            .collect();

        let targets: Vec<UnitKind> = units.all_build_recipes().keys().cloned().collect();

        if builders.is_empty() {
            anyhow::bail!("No builder units found in {}", path.display());
        }
        if targets.is_empty() {
            anyhow::bail!("No buildable target units found in {}", path.display());
        }

        self.source = UnitSource::Real {
            units,
            builders,
            targets,
        };
        Ok(self)
    }

    /// Start a fluent pipeline that will write to `db_path`.
    ///
    /// The pipeline can be run step-by-step or via [`Self::generate`] which
    /// simply calls each stage in order.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// DatasetGenerator::new(GenerationConfig::default())
    ///     .with_units_file(Path::new("plugins/faf-units/data/faf_units.json"))?
    ///     .pipeline(Path::new("data/dataset.db"))?
    ///     .create_schema()?
    ///     .generate_samples()?
    ///     .save_norm()?
    ///     .finish()?;
    /// ```
    pub fn pipeline(self, db_path: &Path) -> Result<DatasetPipeline> {
        let conn = Connection::open(db_path)
            .with_context(|| format!("Failed to open SQLite database at {}", db_path.display()))?;

        Ok(DatasetPipeline {
            generator: self,
            db_path: db_path.to_path_buf(),
            conn,
            stats: NormalizationParams::new(),
        })
    }

    /// Generate the dataset and write it to `db_path`.
    ///
    /// This is a convenience wrapper around [`Self::pipeline`].
    pub fn generate(self, db_path: &Path) -> Result<()> {
        self.pipeline(db_path)?
            .create_schema()?
            .generate_samples()?
            .save_norm()?
            .finish()
    }

    fn generate_sample<R: Rng>(&self, rng: &mut R) -> EcoPlanSample {
        let initial_eco = self.sample_initial_eco(rng);
        let task_count = rng.random_range(1..=self.config.max_tasks.max(1));
        let plan: Vec<BuildTask> = (0..task_count)
            .map(|id| self.sample_build_task(rng, id as u32))
            .collect();

        EcoPlanSample {
            initial_eco,
            plan,
            label: EcoPlanLabel::NotPractical { time_seconds: 0.0 }, // replaced by simulation
        }
    }

    fn sample_build_task<R: Rng>(&self, rng: &mut R, id: u32) -> BuildTask {
        let builder_count = rng.random_range(1..=self.config.max_builders_per_task.max(1));
        let target_count = rng.random_range(1..=self.config.max_targets_per_task.max(1));

        let builders: Vec<UnitEcoStats> = (0..builder_count)
            .map(|_| self.sample_builder(rng))
            .collect();
        let targets: Vec<UnitEcoStats> =
            (0..target_count).map(|_| self.sample_target(rng)).collect();

        BuildTask {
            id,
            start_after: Time::from_raw(1.0),
            builders,
            targets,
        }
    }

    fn sample_builder<R: Rng>(&self, rng: &mut R) -> UnitEcoStats {
        match &self.source {
            UnitSource::Synthetic => random_synthetic_builder(rng),
            UnitSource::Real {
                units, builders, ..
            } => {
                let kind = builders[rng.random_range(0..builders.len())].clone();
                let def = units.def(&kind).expect("builder kind missing from units");
                unit_as_builder(def)
            }
        }
    }

    fn sample_target<R: Rng>(&self, rng: &mut R) -> UnitEcoStats {
        match &self.source {
            UnitSource::Synthetic => random_synthetic_target(rng),
            UnitSource::Real { units, targets, .. } => {
                let kind = targets[rng.random_range(0..targets.len())].clone();
                let def = units.def(&kind).expect("target kind missing from units");
                unit_as_target(def)
            }
        }
    }

    fn sample_initial_eco<R: Rng>(&self, rng: &mut R) -> EcoSnapshot {
        match &self.source {
            UnitSource::Synthetic => random_synthetic_eco_snapshot(rng),
            UnitSource::Real { units, .. } => sample_real_initial_eco(rng, units),
        }
    }
}

/// A fluent, step-by-step pipeline for generating a dataset.
///
/// Each method consumes `self` and returns `Self`, so the stages can be chained:
///
/// ```rust,ignore
/// generator
///     .pipeline(db_path)?
///     .create_schema()?
///     .generate_samples()?
///     .save_norm()?
///     .finish()?;
/// ```
pub struct DatasetPipeline {
    generator: DatasetGenerator,
    db_path: PathBuf,
    conn: Connection,
    stats: NormalizationParams,
}

impl DatasetPipeline {
    /// Create the `samples` and `metadata` tables if they do not exist.
    pub fn create_schema(mut self) -> Result<Self> {
        create_schema(&mut self.conn)?;
        Ok(self)
    }

    /// Generate all configured samples, simulate them, and insert the rows.
    pub fn generate_samples(mut self) -> Result<Self> {
        let tx = self
            .conn
            .transaction()
            .context("Failed to start SQLite transaction")?;
        let mut rng = rand::rng();

        for i in 0..self.generator.config.sample_count {
            let sample = self.generator.generate_sample(&mut rng);
            let task_features = extract_sequence_features(&sample.initial_eco, &sample.plan);
            for task in &task_features {
                self.stats.update(task);
            }

            let label = simulate_label(&sample, self.generator.config.time_limit_seconds);
            insert_sample(&tx, &task_features, &label)?;

            if (i + 1) % 1000 == 0 {
                println!(
                    "Generated {} / {} samples",
                    i + 1,
                    self.generator.config.sample_count
                );
            }
        }

        tx.commit().context("Failed to commit SQLite transaction")?;
        Ok(self)
    }

    /// Persist the normalization parameters computed while sampling.
    pub fn save_norm(self) -> Result<Self> {
        let norm_path = self.db_path.with_extension("norm.json");
        self.stats.save(&norm_path)?;
        Ok(self)
    }

    /// Complete the pipeline and print a summary.
    pub fn finish(self) -> Result<()> {
        println!(
            "Dataset complete: {} samples in {}",
            self.generator.config.sample_count,
            self.db_path.display()
        );
        Ok(())
    }
}

/// Convenience function that generates a synthetic dataset.
pub fn generate_dataset(db_path: &Path, config: GenerationConfig) -> Result<()> {
    DatasetGenerator::new(config).generate(db_path)
}

fn create_schema(conn: &mut Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS samples (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            sequence_features TEXT NOT NULL,
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
    task_features: &[[f64; crate::data::sample::TASK_FEATURE_DIM]],
    label: &EcoPlanLabel,
) -> Result<()> {
    let features_json =
        serde_json::to_string(task_features).context("Failed to serialize sequence features")?;
    let target_time = label.time_seconds();
    let is_practical = label.is_practical() as i32;

    tx.execute(
        "INSERT INTO samples (sequence_features, target_time, is_practical) VALUES (?1, ?2, ?3)",
        [
            &features_json,
            &target_time.to_string(),
            &is_practical.to_string(),
        ],
    )
    .context("Failed to insert sample")?;

    Ok(())
}

fn simulate_label(sample: &EcoPlanSample, time_limit_seconds: f64) -> EcoPlanLabel {
    let dt = StepTime::from_seconds(1).expect("1 second dt is valid");
    // Run the simulator up to 10x the practical threshold so the model can
    // learn degrees of "not practical" instead of a single clipped value.
    let max_sim_time = Time::from_raw(time_limit_seconds * 10.0);
    let queue = build_queue(&sample.initial_eco, sample.plan.clone());

    let mut sim = Simulation::new(queue, dt, Some(max_sim_time));

    while !sim.is_finished() {
        sim.step();
    }

    let final_time = sim.current_time().value();
    if final_time < time_limit_seconds - dt.as_time().value() {
        EcoPlanLabel::Practical {
            time_seconds: final_time,
        }
    } else {
        EcoPlanLabel::NotPractical {
            time_seconds: final_time,
        }
    }
}

// -----------------------------------------------------------------------------
// Synthetic sampling
// -----------------------------------------------------------------------------

fn random_synthetic_eco_snapshot<R: Rng>(rng: &mut R) -> EcoSnapshot {
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

fn random_synthetic_builder<R: Rng>(rng: &mut R) -> UnitEcoStats {
    UnitEcoStats {
        build_power: rng.random_range(1.0..50.0),
        mass_cost: 0.0,
        energy_cost: 0.0,
        build_time: 0.0,
        production_per_second_mass: 0.0,
        production_per_second_energy: 0.0,
        maintenance_consumption_per_second_energy: rng.random_range(0.0..200.0),
        mass_storage: 0.0,
        energy_storage: 0.0,
        unit_id: None,
    }
}

fn random_synthetic_target<R: Rng>(rng: &mut R) -> UnitEcoStats {
    UnitEcoStats {
        build_power: 0.0,
        mass_cost: rng.random_range(1.0..20000.0),
        energy_cost: rng.random_range(1.0..100000.0),
        build_time: rng.random_range(1.0..5000.0),
        production_per_second_mass: rng.random_range(0.0..50.0),
        production_per_second_energy: rng.random_range(0.0..5000.0),
        maintenance_consumption_per_second_energy: rng.random_range(0.0..2000.0),
        mass_storage: rng.random_range(0.0..1000.0),
        energy_storage: rng.random_range(0.0..10000.0),
        unit_id: None,
    }
}

// -----------------------------------------------------------------------------
// Real FAF-unit sampling
// -----------------------------------------------------------------------------

fn unit_as_builder(def: &UnitDef) -> UnitEcoStats {
    UnitEcoStats {
        build_power: def.build_rate(),
        maintenance_consumption_per_second_energy: def.maintenance_consumption_per_second_energy(),
        unit_id: Some(def.display_name.clone()),
        ..Default::default()
    }
}

fn unit_as_target(def: &UnitDef) -> UnitEcoStats {
    UnitEcoStats {
        build_power: 0.0,
        mass_cost: def.cost.mass,
        energy_cost: def.cost.energy,
        build_time: def.cost.build_time,
        production_per_second_mass: def.production_per_second_mass(),
        production_per_second_energy: def.production_per_second_energy(),
        maintenance_consumption_per_second_energy: def.maintenance_consumption_per_second_energy(),
        mass_storage: def.mass_storage(),
        energy_storage: def.energy_storage(),
        unit_id: Some(def.display_name.clone()),
    }
}

fn sample_real_initial_eco<R: Rng>(rng: &mut R, units: &Units) -> EcoSnapshot {
    let commander = units
        .def(&UnitKind::Commander)
        .expect("Commander unit must be defined");

    let mut production_mass = commander.production_per_second_mass();
    let mut production_energy = commander.production_per_second_energy();
    let mut maintenance = commander.maintenance_consumption_per_second_energy();
    let mut mass_storage_cap = commander.mass_storage();
    let mut energy_storage_cap = commander.energy_storage();

    let t1_engineer = UnitKind::Engineer(TechLevel::T1);
    if let Some(eng) = units.def(&t1_engineer) {
        let engineer_count = rng.random_range(1..=5);
        for _ in 0..engineer_count {
            production_mass += eng.production_per_second_mass();
            production_energy += eng.production_per_second_energy();
            maintenance += eng.maintenance_consumption_per_second_energy();
            mass_storage_cap += eng.mass_storage();
            energy_storage_cap += eng.energy_storage();
        }
    }

    let t1_pgen = UnitKind::Pgen(TechLevel::T1);
    if let Some(pgen) = units.def(&t1_pgen) {
        let pgen_count = rng.random_range(1..=6);
        for _ in 0..pgen_count {
            production_energy += pgen.production_per_second_energy();
            maintenance += pgen.maintenance_consumption_per_second_energy();
            energy_storage_cap += pgen.energy_storage();
        }
    }

    let t1_mex = UnitKind::Mex(TechLevel::T1);
    if let Some(mex) = units.def(&t1_mex) {
        let mex_count = rng.random_range(0..=4);
        for _ in 0..mex_count {
            production_mass += mex.production_per_second_mass();
            maintenance += mex.maintenance_consumption_per_second_energy();
            mass_storage_cap += mex.mass_storage();
        }
    }

    EcoSnapshot {
        time: 0.0,
        production_per_second_mass: production_mass,
        production_per_second_energy: production_energy,
        maintenance_consumption_per_second_energy: maintenance,
        mass_drain: 0.0,
        energy_drain: 0.0,
        total_mass_spent: 0.0,
        total_energy_spent: 0.0,
        mass_storage: mass_storage_cap,
        mass_storage_cap,
        energy_storage: energy_storage_cap,
        energy_storage_cap,
    }
}
