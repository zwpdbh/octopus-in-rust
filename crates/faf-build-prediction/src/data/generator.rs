//! Generate a SQLite dataset of simulated build plans and their completion times.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use faf_sim::quantities::{StepTime, Time};
use faf_sim::runtime::{BuildTask, EcoSnapshot, UnitEcoStats};
use faf_sim::sim::Simulation;
use faf_sim::units::{BlueprintLibrary, TechLevel, UnitKind};
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
            max_builders_per_task: 20,
            max_targets_per_task: 5,
        }
    }
}

/// Builder-pattern generator for a `faf-build-prediction` training dataset.
///
/// # Example
///
/// ```rust,ignore
/// DatasetGenerator::new(
///     GenerationConfig::default(),
///     Path::new("plugins/faf-units/data/faf_units.json"),
/// )?
/// .generate(Path::new("data/dataset.db"))?;
/// ```
pub struct DatasetGenerator {
    config: GenerationConfig,
    library: BlueprintLibrary,
    /// Blueprint kinds that can act as builders.
    builders: HashSet<UnitKind>,
    /// Blueprint kinds that can be built as targets.
    targets: HashSet<UnitKind>,
}

impl DatasetGenerator {
    /// Create a generator that samples from the real FAF unit database at `units_file`.
    pub fn new(config: GenerationConfig, units_file: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(units_file)
            .with_context(|| format!("Failed to read units file {}", units_file.display()))?;
        let index: faf_units::DataIndex = serde_json::from_str(&text)
            .with_context(|| format!("Failed to parse units file {}", units_file.display()))?;
        let library = BlueprintLibrary::new(index);

        let builders = library.builder_blueprints(None);
        let targets = library.target_blueprints(None);

        if builders.is_empty() {
            anyhow::bail!("No builder units found in {}", units_file.display());
        }
        if targets.is_empty() {
            anyhow::bail!(
                "No buildable target units found in {}",
                units_file.display()
            );
        }

        Ok(Self {
            config,
            library,
            builders,
            targets,
        })
    }

    /// Start a fluent pipeline that will write to `db_path`.
    ///
    /// The pipeline can be run step-by-step or via [`Self::generate`] which
    /// simply calls each stage in order.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// DatasetGenerator::new(
    ///     GenerationConfig::default(),
    ///     Path::new("plugins/faf-units/data/faf_units.json"),
    /// )?
    /// .pipeline(Path::new("data/dataset.db"))?
    /// .create_schema()?
    /// .generate_samples()?
    /// .save_norm()?
    /// .finish()?;
    /// ```
    pub fn pipeline(self, db_path: &Path) -> Result<DatasetPipeline> {
        let conn = Connection::open(db_path)
            .with_context(|| format!("Failed to open SQLite database at {}", db_path.display()))?;

        Ok(DatasetPipeline {
            generator: self,
            db_path: db_path.to_path_buf(),
            conn,
            stats: NormalizationParams::new(),
            practical_count: 0,
            not_practical_count: 0,
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
        let kind = sample_from_set(&self.builders, rng);
        self.library
            .to_unit_eco_stats(&kind, true)
            .expect("builder kind missing from library")
    }

    fn sample_target<R: Rng>(&self, rng: &mut R) -> UnitEcoStats {
        let kind = sample_from_set(&self.targets, rng);
        self.library
            .to_unit_eco_stats(&kind, false)
            .expect("target kind missing from library")
    }

    fn sample_initial_eco<R: Rng>(&self, rng: &mut R) -> EcoSnapshot {
        sample_real_initial_eco(rng, &self.library)
    }
}

/// Sample a random element from a non-empty `HashSet`.
fn sample_from_set<R: Rng>(set: &HashSet<UnitKind>, rng: &mut R) -> UnitKind {
    let idx = rng.random_range(0..set.len());
    set.iter().nth(idx).expect("non-empty set").clone()
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
    practical_count: usize,
    not_practical_count: usize,
}

impl DatasetPipeline {
    /// Drop and recreate the `samples` and `metadata` tables.
    ///
    /// This ensures each dataset generation starts with a clean database so
    /// training always uses only the most recently generated samples.
    pub fn create_schema(mut self) -> Result<Self> {
        println!(
            "Preparing fresh dataset at {} (existing tables will be dropped)...",
            self.db_path.display()
        );
        create_schema(&mut self.conn)?;
        Ok(self)
    }

    /// Generate all configured samples, simulate them, and insert the rows.
    pub fn generate_samples(mut self) -> Result<Self> {
        let config = self.generator.config;
        println!("Generating dataset:");
        println!("  samples: {}", config.sample_count);
        println!("  time_limit_seconds: {}", config.time_limit_seconds);
        println!("  max_tasks: {}", config.max_tasks);
        println!("  max_builders_per_task: {}", config.max_builders_per_task);
        println!("  max_targets_per_task: {}", config.max_targets_per_task);
        println!("  builder blueprints: {}", self.generator.builders.len());
        println!("  target blueprints: {}", self.generator.targets.len());

        let tx = self
            .conn
            .transaction()
            .context("Failed to start SQLite transaction")?;
        let mut rng = rand::rng();
        let sample_count = config.sample_count;
        let time_limit = config.time_limit_seconds;
        let generator = &self.generator;
        let stats = &mut self.stats;

        (0..sample_count)
            .try_for_each(|i| -> Result<()> {
                let sample = generator.generate_sample(&mut rng);
                let task_features = extract_sequence_features(&sample.initial_eco, &sample.plan);
                task_features.iter().for_each(|task| stats.update(task));

                let label = simulate_label(&sample, time_limit);
                if label.is_practical() {
                    self.practical_count += 1;
                } else {
                    self.not_practical_count += 1;
                }
                insert_sample(&tx, &task_features, &label)
                    .with_context(|| format!("Failed to insert sample {}", i + 1))?;

                if (i + 1) % 1000 == 0 {
                    println!("Generated {} / {} samples", i + 1, sample_count);
                }
                Ok(())
            })
            .context("Failed to generate samples")?;

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
        let total = self.generator.config.sample_count;
        let norm_path = self.db_path.with_extension("norm.json");
        println!(
            "Dataset complete: {} samples written to {}",
            total,
            self.db_path.display()
        );
        println!("  practical: {}", self.practical_count);
        println!("  not_practical: {}", self.not_practical_count);
        println!("  normalization saved to {}", norm_path.display());
        Ok(())
    }
}

/// Convenience function that generates a dataset from real FAF units.
pub fn generate_dataset(db_path: &Path, config: GenerationConfig, units_file: &Path) -> Result<()> {
    DatasetGenerator::new(config, units_file)?.generate(db_path)
}

fn create_schema(conn: &mut Connection) -> Result<()> {
    conn.execute("DROP TABLE IF EXISTS samples", [])
        .context("Failed to drop existing samples table")?;
    conn.execute("DROP TABLE IF EXISTS metadata", [])
        .context("Failed to drop existing metadata table")?;

    conn.execute(
        "CREATE TABLE samples (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            sequence_features TEXT NOT NULL,
            target_time REAL NOT NULL,
            is_practical INTEGER NOT NULL
        )",
        [],
    )
    .context("Failed to create samples table")?;

    conn.execute(
        "CREATE TABLE metadata (
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
// Real FAF-unit sampling
// -----------------------------------------------------------------------------

fn sample_real_initial_eco<R: Rng>(rng: &mut R, library: &BlueprintLibrary) -> EcoSnapshot {
    let mut production_mass = library.production_per_second_mass(&UnitKind::Commander);
    let mut production_energy = library.production_per_second_energy(&UnitKind::Commander);
    let mut maintenance = library.maintenance_consumption_per_second_energy(&UnitKind::Commander);
    let mut mass_storage_cap = library.mass_storage(&UnitKind::Commander);
    let mut energy_storage_cap = library.energy_storage(&UnitKind::Commander);

    let t1_engineer = UnitKind::Engineer(TechLevel::T1);
    if library.entity_for_kind(&t1_engineer).is_some() {
        let engineer_count = rng.random_range(1..=5);
        for _ in 0..engineer_count {
            production_mass += library.production_per_second_mass(&t1_engineer);
            production_energy += library.production_per_second_energy(&t1_engineer);
            maintenance += library.maintenance_consumption_per_second_energy(&t1_engineer);
            mass_storage_cap += library.mass_storage(&t1_engineer);
            energy_storage_cap += library.energy_storage(&t1_engineer);
        }
    }

    let t1_pgen = UnitKind::Pgen(TechLevel::T1);
    if library.entity_for_kind(&t1_pgen).is_some() {
        let pgen_count = rng.random_range(1..=6);
        for _ in 0..pgen_count {
            production_energy += library.production_per_second_energy(&t1_pgen);
            maintenance += library.maintenance_consumption_per_second_energy(&t1_pgen);
            energy_storage_cap += library.energy_storage(&t1_pgen);
        }
    }

    let t1_mex = UnitKind::Mex(TechLevel::T1);
    if library.entity_for_kind(&t1_mex).is_some() {
        let mex_count = rng.random_range(0..=4);
        for _ in 0..mex_count {
            production_mass += library.production_per_second_mass(&t1_mex);
            maintenance += library.maintenance_consumption_per_second_energy(&t1_mex);
            mass_storage_cap += library.mass_storage(&t1_mex);
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
