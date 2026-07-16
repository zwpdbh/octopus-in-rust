//! The `dataset` command: generate training data or inspect its distribution.

use faf_build_prediction::{print_time_distribution, DatasetGenerator, GenerationConfig};

use crate::command_line::DatasetMode;

/// Entry point for the `dataset` command.
pub fn run(mode: DatasetMode) {
    match mode {
        DatasetMode::Generate {
            output,
            samples,
            max_builders_per_task,
            max_targets_per_task,
            units_file,
        } => {
            let generator = match DatasetGenerator::new(
                GenerationConfig {
                    sample_count: samples,
                    max_builders_per_task,
                    max_targets_per_task,
                },
                &units_file,
            ) {
                Ok(g) => g,
                Err(e) => {
                    eprintln!("Failed to load units file: {e}");
                    std::process::exit(1);
                }
            };

            if let Err(e) = generator.generate(&output) {
                eprintln!("Dataset generation failed: {e}");
                std::process::exit(1);
            }
        }
        DatasetMode::Histogram { dataset } => {
            if let Err(e) = print_time_distribution(&dataset) {
                eprintln!("Histogram failed: {e}");
                std::process::exit(1);
            }
        }
    }
}
