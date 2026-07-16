//! Utilities for inspecting the distribution of simulated completion times.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};

/// Print an ASCII histogram and summary statistics for completion times in a
/// generated dataset.
pub fn print_time_distribution(db_path: &Path) -> Result<()> {
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("Failed to open dataset {}", db_path.display()))?;
    let mut stmt = conn
        .prepare("SELECT target_time FROM samples")
        .context("Failed to prepare time query")?;
    let times: Vec<f64> = stmt
        .query_map([], |row| row.get(0))
        .context("Failed to read completion times")?
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to collect completion times")?;

    if times.is_empty() {
        println!("No samples in dataset.");
        return Ok(());
    }

    print_ascii_histogram(&times);
    Ok(())
}

fn print_ascii_histogram(times: &[f64]) {
    let count = times.len();
    let mut sorted = times.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let min = sorted.first().copied().unwrap_or(0.0);
    let max = sorted.last().copied().unwrap_or(0.0);
    let mean = times.iter().sum::<f64>() / count as f64;
    let median = percentile(&sorted, 0.5);
    let p10 = percentile(&sorted, 0.1);
    let p25 = percentile(&sorted, 0.25);
    let p75 = percentile(&sorted, 0.75);
    let p90 = percentile(&sorted, 0.9);

    println!("Completion time distribution ({} samples):", count);
    println!("  min:    {:>8.1} s", min);
    println!("  p10:    {:>8.1} s", p10);
    println!("  p25:    {:>8.1} s", p25);
    println!("  median: {:>8.1} s", median);
    println!("  mean:   {:>8.1} s", mean);
    println!("  p75:    {:>8.1} s", p75);
    println!("  p90:    {:>8.1} s", p90);
    println!("  max:    {:>8.1} s", max);
    println!();

    let bins = 20;
    let log_min = min.max(1.0).ln();
    let log_max = max.ln();
    let mut counts = vec![0usize; bins];

    if (log_max - log_min).abs() < 1e-9 {
        // All times are effectively equal; put everything in the first bin.
        counts[0] = count;
    } else {
        for &t in times {
            let log_t = t.max(1.0).ln();
            let bin = ((log_t - log_min) / (log_max - log_min) * (bins - 1) as f64) as usize;
            counts[bin.min(bins - 1)] += 1;
        }
    }

    let max_count = counts.iter().copied().max().unwrap_or(1).max(1);
    let max_bar_width = 40;

    println!("Log-spaced histogram:");
    for i in 0..bins {
        let lower = if i == 0 {
            min
        } else {
            exp_log_bin(log_min, log_max, bins, i)
        };
        let upper = if i == bins - 1 {
            max
        } else {
            exp_log_bin(log_min, log_max, bins, i + 1)
        };
        let bar_len = (counts[i] as f64 / max_count as f64 * max_bar_width as f64) as usize;
        let bar = "#".repeat(bar_len);
        println!(
            "{:>8.1} s - {:>8.1} s | {:>5} {}",
            lower, upper, counts[i], bar
        );
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (p * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn exp_log_bin(log_min: f64, log_max: f64, bins: usize, i: usize) -> f64 {
    let fraction = i as f64 / bins as f64;
    (log_min + fraction * (log_max - log_min)).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_selects_correct_element() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile(&values, 0.0), 1.0);
        assert_eq!(percentile(&values, 0.5), 3.0);
        assert_eq!(percentile(&values, 1.0), 5.0);
    }
}
