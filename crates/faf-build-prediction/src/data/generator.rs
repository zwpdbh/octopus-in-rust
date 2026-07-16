//! Generate a SQLite dataset of simulated build plans and their completion times.

use std::collections::HashSet;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use faf_sim::quantities::Time;
use faf_sim::runtime::{BuildTask, EcoSnapshot, UnitEcoStats};
use faf_sim::units::{BlueprintLibrary, TechLevel, UnitKind};
use rand::{Rng, RngExt};
use rusqlite::{Connection, Transaction};

use crate::data::normalize::{Collecting, NormalizationParams, Ready};
use crate::data::sample::{
    extract_sequence_features, EcoPlanSample, Simulated, Unsimulated, TASK_FEATURE_DIM,
};
use std::marker::PhantomData;

/// Configuration controlling dataset generation.
#[derive(Debug, Clone, Copy)]
pub struct GenerationConfig {
    /// Number of labeled samples to generate.
    pub sample_count: usize,
    /// Maximum number of builders assigned to a single task.
    pub max_builders_per_task: usize,
    /// Maximum number of targets inside a single task.
    pub max_targets_per_task: usize,
}

impl Default for GenerationConfig {
    fn default() -> Self {
        Self {
            sample_count: 10_000,
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
    pub fn pipeline(self, db_path: &Path) -> Result<DatasetPipeline<PipelineNew>> {
        let conn = Connection::open(db_path)
            .with_context(|| format!("Failed to open SQLite database at {}", db_path.display()))?;

        Ok(DatasetPipeline {
            generator: self,
            db_path: db_path.to_path_buf(),
            conn,
            stats: NormalizationParams::new(),
            _stage: PhantomData,
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

    fn generate_sample<R: Rng>(&self, rng: &mut R) -> EcoPlanSample<Unsimulated> {
        // Sample the task first so the initial economy can be matched to it.
        // This predictor is trained on single-task plans only.
        let task = self.sample_build_task(rng, 0);
        let initial_eco = self.sample_initial_eco(rng, &task);
        EcoPlanSample::new(initial_eco, vec![task])
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

    fn sample_initial_eco<R: Rng>(&self, rng: &mut R, task: &BuildTask) -> EcoSnapshot {
        sample_real_initial_eco(rng, &self.library, task)
    }
}

/// Sample a random element from a non-empty `HashSet`.
fn sample_from_set<R: Rng>(set: &HashSet<UnitKind>, rng: &mut R) -> UnitKind {
    let idx = rng.random_range(0..set.len());
    set.iter().nth(idx).expect("non-empty set").clone()
}

/// Stage markers for [`DatasetPipeline`].
pub struct PipelineNew;
pub struct SchemaCreated;
pub struct SamplesGenerated;
pub struct NormSaved;

/// Internal trait linking each pipeline stage to the normalization state it
/// carries (`Collecting` before samples are generated, `Ready` after).
pub trait PipelineStage {
    type NormState;
}

impl PipelineStage for PipelineNew {
    type NormState = Collecting;
}

impl PipelineStage for SchemaCreated {
    type NormState = Collecting;
}

impl PipelineStage for SamplesGenerated {
    type NormState = Ready;
}

impl PipelineStage for NormSaved {
    type NormState = Ready;
}

/// A type-state pipeline for generating a dataset.
///
/// The compiler enforces the legal order:
///
/// ```rust,ignore
/// generator
///     .pipeline(db_path)?
///     .create_schema()?
///     .generate_samples()?
///     .save_norm()?
///     .finish()?;
/// ```
pub struct DatasetPipeline<Stage: PipelineStage> {
    generator: DatasetGenerator,
    db_path: PathBuf,
    conn: Connection,
    stats: NormalizationParams<Stage::NormState>,
    _stage: PhantomData<Stage>,
}

impl DatasetPipeline<PipelineNew> {
    /// Drop and recreate the `samples` and `metadata` tables.
    ///
    /// This ensures each dataset generation starts with a clean database so
    /// training always uses only the most recently generated samples.
    pub fn create_schema(mut self) -> Result<DatasetPipeline<SchemaCreated>> {
        println!(
            "Preparing fresh dataset at {} (existing tables will be dropped)...",
            self.db_path.display()
        );
        create_schema(&mut self.conn)?;
        Ok(DatasetPipeline {
            generator: self.generator,
            db_path: self.db_path,
            conn: self.conn,
            stats: self.stats,
            _stage: PhantomData,
        })
    }
}

impl DatasetPipeline<SchemaCreated> {
    /// Generate all configured samples, simulate them, and insert the rows.
    pub fn generate_samples(mut self) -> Result<DatasetPipeline<SamplesGenerated>> {
        let config = self.generator.config;
        println!("Generating dataset:");
        println!("  samples: {}", config.sample_count);
        println!("  tasks per plan: 1 (single-task predictor)");
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
        let generator = &self.generator;
        let stats = &mut self.stats;

        for i in 0..sample_count {
            SamplePipeline {
                generator,
                rng: &mut rng,
                tx: &tx,
                stats,
            }
            .generate_sample()
            .simulate()
            .extract_sequence_features()
            .insert_sample()
            .with_context(|| format!("Failed to insert sample {}", i + 1))?;

            if (i + 1) % 1000 == 0 {
                println!("Generated {} / {} samples", i + 1, sample_count);
            }
        }

        tx.commit().context("Failed to commit SQLite transaction")?;
        let stats = self.stats.finalize();
        Ok(DatasetPipeline {
            generator: self.generator,
            db_path: self.db_path,
            conn: self.conn,
            stats,
            _stage: PhantomData,
        })
    }
}

impl DatasetPipeline<SamplesGenerated> {
    /// Persist the normalization parameters computed while sampling.
    pub fn save_norm(self) -> Result<DatasetPipeline<NormSaved>> {
        let norm_path = self.db_path.with_extension("norm.json");
        self.stats.save(&norm_path)?;
        Ok(DatasetPipeline {
            generator: self.generator,
            db_path: self.db_path,
            conn: self.conn,
            stats: self.stats,
            _stage: PhantomData,
        })
    }
}

impl DatasetPipeline<NormSaved> {
    /// Complete the pipeline and print a summary.
    pub fn finish(self) -> Result<()> {
        let total = self.generator.config.sample_count;
        let norm_path = self.db_path.with_extension("norm.json");
        println!(
            "Dataset complete: {} samples written to {}",
            total,
            self.db_path.display()
        );
        println!("  normalization saved to {}", norm_path.display());
        Ok(())
    }
}

/// Per-sample type-state pipeline that enforces the legal transition order:
///
/// `generate_sample -> simulate -> extract_sequence_features -> insert_sample`.
///
/// Each stage returns a distinct type so the compiler rejects out-of-order calls.
struct SamplePipeline<'a, 'conn, R: Rng> {
    generator: &'a DatasetGenerator,
    rng: &'a mut R,
    tx: &'a Transaction<'conn>,
    stats: &'a mut NormalizationParams<Collecting>,
}

impl<'a, 'conn, R: Rng> SamplePipeline<'a, 'conn, R> {
    fn generate_sample(&'a mut self) -> UnsimulatedSample<'a, 'conn, R> {
        let sample = self.generator.generate_sample(self.rng);
        UnsimulatedSample { sample, ctx: self }
    }
}

struct UnsimulatedSample<'a, 'conn, R: Rng> {
    sample: EcoPlanSample<Unsimulated>,
    ctx: &'a mut SamplePipeline<'a, 'conn, R>,
}

impl<'a, 'conn, R: Rng> UnsimulatedSample<'a, 'conn, R> {
    fn simulate(self) -> SimulatedSample<'a, 'conn, R> {
        let sample = self.sample.simulate();
        SimulatedSample {
            sample,
            ctx: self.ctx,
        }
    }
}

struct SimulatedSample<'a, 'conn, R: Rng> {
    sample: EcoPlanSample<Simulated>,
    ctx: &'a mut SamplePipeline<'a, 'conn, R>,
}

impl<'a, 'conn, R: Rng> SimulatedSample<'a, 'conn, R> {
    fn extract_sequence_features(self) -> FeaturedSample<'a, 'conn, R> {
        let features = extract_sequence_features(&self.sample.initial_eco, &self.sample.plan);
        features.iter().for_each(|task| self.ctx.stats.update(task));
        FeaturedSample {
            sample: self.sample,
            features,
            ctx: self.ctx,
        }
    }
}

struct FeaturedSample<'a, 'conn, R: Rng> {
    sample: EcoPlanSample<Simulated>,
    features: Vec<[f64; TASK_FEATURE_DIM]>,
    ctx: &'a mut SamplePipeline<'a, 'conn, R>,
}

impl<'a, 'conn, R: Rng> FeaturedSample<'a, 'conn, R> {
    fn insert_sample(self) -> Result<()> {
        insert_sample(self.ctx.tx, &self.features, &self.sample)
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
            target_time REAL NOT NULL
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

/// Insert a simulated sample into the database.
///
/// The `EcoPlanSample<Simulated>` type guarantees at compile time that the
/// sample has been simulated and carries a real completion time; un-simulated
/// samples cannot be passed here.
fn insert_sample(
    tx: &Transaction,
    task_features: &[[f64; crate::data::sample::TASK_FEATURE_DIM]],
    sample: &EcoPlanSample,
) -> Result<()> {
    let features_json =
        serde_json::to_string(task_features).context("Failed to serialize sequence features")?;
    let target_time = sample.time_seconds();

    tx.execute(
        "INSERT INTO samples (sequence_features, target_time) VALUES (?1, ?2)",
        [&features_json, &target_time.to_string()],
    )
    .context("Failed to insert sample")?;

    Ok(())
}

// -----------------------------------------------------------------------------
// Real FAF-unit sampling
// -----------------------------------------------------------------------------

fn sample_real_initial_eco<R: Rng>(
    rng: &mut R,
    library: &BlueprintLibrary,
    task: &BuildTask,
) -> EcoSnapshot {
    let t1_engineer = UnitKind::Engineer(TechLevel::T1);
    let t1_pgen = UnitKind::Pgen(TechLevel::T1);
    let t1_mex = UnitKind::Mex(TechLevel::T1);

    // If any required T1 unit is missing, fall back to ACU-only.
    if library.entity_for_kind(&t1_engineer).is_none()
        || library.entity_for_kind(&t1_pgen).is_none()
        || library.entity_for_kind(&t1_mex).is_none()
    {
        return EcoSnapshotBuilder::new(library, rng).add_acu().build();
    }

    EcoSnapshotBuilder::new(library, rng)
        .add_acu()
        .add_engineers(0..=3)
        .add_power_for(task, 0..=3)
        .add_mass_for(task, 0..=2)
        .build()
}

/// Fluent builder for constructing a feasible starting economy.
///
/// The builder accumulates production, maintenance, and storage from discrete
/// FAF units. It is intentionally not type-state: the ordering is enforced by
/// convention, which keeps the code concise for this linear, single-output
/// workflow.
struct EcoSnapshotBuilder<'a, R: Rng> {
    library: &'a BlueprintLibrary,
    rng: &'a mut R,
    production_mass: f64,
    production_energy: f64,
    maintenance: f64,
    mass_storage_cap: f64,
    energy_storage_cap: f64,
}

impl<'a, R: Rng> EcoSnapshotBuilder<'a, R> {
    fn new(library: &'a BlueprintLibrary, rng: &'a mut R) -> Self {
        Self {
            library,
            rng,
            production_mass: 0.0,
            production_energy: 0.0,
            maintenance: 0.0,
            mass_storage_cap: 0.0,
            energy_storage_cap: 0.0,
        }
    }

    fn add_acu(mut self) -> Self {
        self.production_mass += self
            .library
            .production_per_second_mass(&UnitKind::Commander);
        self.production_energy += self
            .library
            .production_per_second_energy(&UnitKind::Commander);
        self.maintenance += self
            .library
            .maintenance_consumption_per_second_energy(&UnitKind::Commander);
        self.mass_storage_cap += self.library.mass_storage(&UnitKind::Commander);
        self.energy_storage_cap += self.library.energy_storage(&UnitKind::Commander);
        self
    }

    fn add_engineers(mut self, count_range: RangeInclusive<usize>) -> Self {
        let kind = UnitKind::Engineer(TechLevel::T1);
        if self.library.entity_for_kind(&kind).is_none() {
            return self;
        }
        let count = self.rng.random_range(count_range);
        for _ in 0..count {
            self.production_mass += self.library.production_per_second_mass(&kind);
            self.production_energy += self.library.production_per_second_energy(&kind);
            self.maintenance += self
                .library
                .maintenance_consumption_per_second_energy(&kind);
            self.mass_storage_cap += self.library.mass_storage(&kind);
            self.energy_storage_cap += self.library.energy_storage(&kind);
        }
        self
    }

    fn add_power_for(mut self, task: &BuildTask, surplus_range: RangeInclusive<usize>) -> Self {
        let kind = UnitKind::Pgen(TechLevel::T1);
        if self.library.entity_for_kind(&kind).is_none() {
            return self;
        }

        let (_, energy_drain, builder_maintenance) = task_drain_rates(task);
        let energy_prod = self.library.production_per_second_energy(&kind);
        let unit_maintenance = self
            .library
            .maintenance_consumption_per_second_energy(&kind);
        let storage = self.library.energy_storage(&kind);

        let mut required = 0;
        while self.production_energy - self.maintenance - builder_maintenance - energy_drain < 0.0 {
            self.production_energy += energy_prod;
            self.maintenance += unit_maintenance;
            self.energy_storage_cap += storage;
            required += 1;
            if required > 100 {
                break; // safety guard
            }
        }

        let surplus = self.rng.random_range(surplus_range);
        for _ in 0..surplus {
            self.production_energy += energy_prod;
            self.maintenance += unit_maintenance;
            self.energy_storage_cap += storage;
        }
        self
    }

    fn add_mass_for(mut self, task: &BuildTask, surplus_range: RangeInclusive<usize>) -> Self {
        let kind = UnitKind::Mex(TechLevel::T1);
        if self.library.entity_for_kind(&kind).is_none() {
            return self;
        }

        let (mass_drain, _, _) = task_drain_rates(task);
        let mass_prod = self.library.production_per_second_mass(&kind);
        let unit_maintenance = self
            .library
            .maintenance_consumption_per_second_energy(&kind);
        let storage = self.library.mass_storage(&kind);

        let mut required = 0;
        while self.production_mass - mass_drain < 0.0 {
            self.production_mass += mass_prod;
            self.maintenance += unit_maintenance;
            self.mass_storage_cap += storage;
            required += 1;
            if required > 100 {
                break; // safety guard
            }
        }

        let surplus = self.rng.random_range(surplus_range);
        for _ in 0..surplus {
            self.production_mass += mass_prod;
            self.maintenance += unit_maintenance;
            self.mass_storage_cap += storage;
        }
        self
    }

    fn build(self) -> EcoSnapshot {
        EcoSnapshot {
            time: 0.0,
            production_per_second_mass: self.production_mass,
            production_per_second_energy: self.production_energy,
            maintenance_consumption_per_second_energy: self.maintenance,
            mass_drain: 0.0,
            energy_drain: 0.0,
            total_mass_spent: 0.0,
            total_energy_spent: 0.0,
            mass_storage: self.mass_storage_cap,
            mass_storage_cap: self.mass_storage_cap,
            energy_storage: self.energy_storage_cap,
            energy_storage_cap: self.energy_storage_cap,
        }
    }
}

fn task_drain_rates(task: &BuildTask) -> (f64, f64, f64) {
    let build_power: f64 = task.builders.iter().map(|b| b.build_power).sum();
    let builder_maintenance: f64 = task
        .builders
        .iter()
        .map(|b| b.maintenance_consumption_per_second_energy)
        .sum();
    let first_target = task.targets.first();
    let first_mass_cost = first_target.map(|t| t.mass_cost).unwrap_or(0.0);
    let first_energy_cost = first_target.map(|t| t.energy_cost).unwrap_or(0.0);
    let first_build_time = first_target.map(|t| t.build_time).unwrap_or(1.0).max(1.0);

    let mass_drain = first_mass_cost / first_build_time * build_power;
    let energy_drain = first_energy_cost / first_build_time * build_power;

    (mass_drain, energy_drain, builder_maintenance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use faf_sim::quantities::Time;
    use faf_sim::runtime::UnitEcoStats;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn test_library() -> BlueprintLibrary {
        let candidates = [
            Path::new("plugins/faf-units/data/faf_units.json").to_path_buf(),
            Path::new("../../plugins/faf-units/data/faf_units.json").to_path_buf(),
        ];
        let path = candidates
            .iter()
            .find(|p| p.exists())
            .expect("units file not found in expected locations");
        let text = std::fs::read_to_string(path).expect("failed to read units file");
        let index: faf_units::DataIndex =
            serde_json::from_str(&text).expect("failed to parse units");
        BlueprintLibrary::new(index)
    }

    fn test_task() -> BuildTask {
        BuildTask {
            id: 0,
            start_after: Time::from_raw(1.0),
            builders: vec![UnitEcoStats {
                build_power: 10.0,
                maintenance_consumption_per_second_energy: 0.0,
                ..Default::default()
            }],
            targets: vec![UnitEcoStats {
                mass_cost: 200.0,
                energy_cost: 1000.0,
                build_time: 200.0,
                ..Default::default()
            }],
        }
    }

    #[test]
    fn initial_eco_supports_task() {
        let library = test_library();
        let task = test_task();
        let mut rng = StdRng::seed_from_u64(42);

        for _ in 0..20 {
            let eco = sample_real_initial_eco(&mut rng, &library, &task);
            let net_energy = eco.production_per_second_energy
                - eco.maintenance_consumption_per_second_energy
                - 0.0; // builder maintenance is zero in this task
            let first_target = task.targets.first().unwrap();
            let mass_drain = first_target.mass_cost / first_target.build_time * 10.0;
            let net_mass = eco.production_per_second_mass - mass_drain;

            assert!(
                net_energy > 0.0,
                "net energy must be positive, got {net_energy}"
            );
            assert!(
                net_mass >= 0.0,
                "net mass must be non-negative, got {net_mass}"
            );
        }
    }
}
