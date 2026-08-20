//! Byte-level transfer progress, shared by sync (download) and upload.
//!
//! [`TransferMeter`] lives inside the transfer loops: feed it chunk sizes
//! with [`TransferMeter::add`] and it decides when a [`TransferUpdate`] is
//! worth emitting (throttled to ~10/s, plus on completion), tracking a
//! smoothed transfer speed along the way. Frontends just render the updates.

use std::time::{Duration, Instant};

/// Minimum interval between progress emissions.
const EMIT_INTERVAL: Duration = Duration::from_millis(100);

/// A snapshot of byte-level transfer progress within one plan (channel).
#[derive(Debug, Clone, Copy)]
pub struct TransferUpdate {
    /// Bytes transferred so far.
    pub done_bytes: u64,
    /// Total bytes to transfer.
    pub total_bytes: u64,
    /// Smoothed transfer speed in bytes per second.
    pub bytes_per_sec: f64,
}

impl TransferUpdate {
    /// Completion in percent (0–100); 100 when there is nothing to transfer.
    pub fn percent(&self) -> f64 {
        if self.total_bytes == 0 {
            100.0
        } else {
            self.done_bytes as f64 / self.total_bytes as f64 * 100.0
        }
    }
}

/// Tracks cumulative bytes of one transfer plan and computes a smoothed
/// speed (EMA), throttling update emission to [`EMIT_INTERVAL`].
pub struct TransferMeter {
    total_bytes: u64,
    done_bytes: u64,
    started: Instant,
    last_emit: Instant,
    last_emit_bytes: u64,
    /// EMA of the instantaneous bytes/sec between emissions.
    speed: f64,
}

impl TransferMeter {
    /// Start metering a plan of `total_bytes`.
    pub fn new(total_bytes: u64) -> Self {
        let now = Instant::now();
        Self {
            total_bytes,
            done_bytes: 0,
            started: now,
            last_emit: now,
            last_emit_bytes: 0,
            speed: 0.0,
        }
    }

    /// Record `n` newly transferred bytes. Returns an update when it's time
    /// to emit one: every [`EMIT_INTERVAL`], and always on completion.
    pub fn add(&mut self, n: u64) -> Option<TransferUpdate> {
        self.done_bytes += n;
        let now = Instant::now();
        let complete = self.total_bytes > 0 && self.done_bytes >= self.total_bytes;
        if !complete && now.duration_since(self.last_emit) < EMIT_INTERVAL {
            return None;
        }
        let dt = now.duration_since(self.last_emit).as_secs_f64();
        if dt > 0.0 {
            let instant = (self.done_bytes - self.last_emit_bytes) as f64 / dt;
            self.speed = if self.speed <= 0.0 {
                instant
            } else {
                0.7 * self.speed + 0.3 * instant
            };
        }
        self.last_emit = now;
        self.last_emit_bytes = self.done_bytes;
        Some(self.update())
    }

    /// Current snapshot, unthrottled (e.g. at a file boundary).
    pub fn update(&self) -> TransferUpdate {
        // Before the first throttled emission there is no EMA yet; fall back
        // to the average since the plan started.
        let speed = if self.speed > 0.0 {
            self.speed
        } else {
            let dt = self.started.elapsed().as_secs_f64();
            if dt > 0.0 {
                self.done_bytes as f64 / dt
            } else {
                0.0
            }
        };
        TransferUpdate {
            done_bytes: self.done_bytes,
            total_bytes: self.total_bytes,
            bytes_per_sec: speed,
        }
    }
}

/// Bundles a [`TransferMeter`] with a run's progress callback, so transfer
/// loops take a single extra parameter instead of two.
pub struct ProgressReporter<'a, E> {
    meter: TransferMeter,
    progress: &'a mut dyn FnMut(E),
    wrap: fn(TransferUpdate) -> E,
}

impl<'a, E> ProgressReporter<'a, E> {
    /// `wrap` turns a [`TransferUpdate`] into the run's event enum variant.
    pub fn new(
        total_bytes: u64,
        progress: &'a mut dyn FnMut(E),
        wrap: fn(TransferUpdate) -> E,
    ) -> Self {
        Self {
            meter: TransferMeter::new(total_bytes),
            progress,
            wrap,
        }
    }

    /// Record `n` transferred bytes, emitting an event when one is due.
    pub fn add(&mut self, n: u64) {
        if let Some(update) = self.meter.add(n) {
            (self.progress)((self.wrap)(update));
        }
    }

    /// Emit the current snapshot unconditionally (e.g. at a file boundary).
    pub fn snapshot(&mut self) {
        let update = self.meter.update();
        (self.progress)((self.wrap)(update));
    }

    /// Forward a non-byte event to the callback.
    pub fn emit(&mut self, event: E) {
        (self.progress)(event);
    }
}

/// Human-readable transfer speed, e.g. `8.5 MB/s`.
pub fn format_speed(bytes_per_sec: f64) -> String {
    if bytes_per_sec >= 1e6 {
        format!("{:.1} MB/s", bytes_per_sec / 1e6)
    } else if bytes_per_sec >= 1e3 {
        format!("{:.0} KB/s", bytes_per_sec / 1e3)
    } else {
        format!("{:.0} B/s", bytes_per_sec)
    }
}

/// Human-readable byte count, e.g. `123.4 MB`.
pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1e6)
    } else if bytes >= 1_000 {
        format!("{:.0} KB", bytes as f64 / 1e3)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_handles_zero_total() {
        let update = TransferUpdate {
            done_bytes: 0,
            total_bytes: 0,
            bytes_per_sec: 0.0,
        };
        assert_eq!(update.percent(), 100.0);
    }

    #[test]
    fn completion_always_emits() {
        let mut meter = TransferMeter::new(10);
        assert!(meter.add(10).is_some());
    }

    #[test]
    fn formatting() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2_500), "2 KB");
        assert_eq!(format_bytes(1_500_000), "1.5 MB");
        assert_eq!(format_speed(8_500_000.0), "8.5 MB/s");
        assert_eq!(format_speed(2_500.0), "2 KB/s");
        assert_eq!(format_speed(100.0), "100 B/s");
    }
}
