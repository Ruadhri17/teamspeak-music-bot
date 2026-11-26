//! Audio statistics of the client, logged every `logging.metrics_interval`: the frames that
//! went out and how long encoding one took, how late the 20 ms clock was, and how often the
//! buffer ran dry in the middle of a stream. Nothing is logged while the bot is silent.

use std::time::Duration;

/// A tick later than this counts as late, a quarter of a frame.
pub const LATE: Duration = Duration::from_millis(5);

#[derive(Debug, Default)]
pub struct AudioMetrics {
    /// How long each frame sent since the last report took to encode.
    encode: Vec<Duration>,
    late: u64,
    max_late: Duration,
    underruns: u64,
    resyncs: u64,
}

impl AudioMetrics {
    pub fn tick(&mut self, late: Duration) {
        if late > LATE {
            self.late += 1;
        }
        self.max_late = self.max_late.max(late);
    }

    /// A frame went out, `encode` is how long its volume and Opus took.
    pub fn sent(&mut self, encode: Duration) {
        self.encode.push(encode);
    }

    /// The buffer was empty while a stream that has not ended was playing.
    pub fn underrun(&mut self) {
        self.underruns += 1;
    }

    /// The clock was so late that it started over.
    pub fn resync(&mut self) {
        self.resyncs += 1;
    }

    /// The line for the log since the last report, which starts counting again.
    /// `None` when no frame went out and nothing went wrong.
    pub fn report(&mut self) -> Option<String> {
        let metrics = std::mem::take(self);
        if metrics.encode.is_empty() && metrics.underruns == 0 && metrics.resyncs == 0 {
            return None;
        }
        let mut encode = metrics.encode;
        encode.sort_unstable();
        let percentile = |p: usize| {
            encode
                .get((encode.len() * p / 100).min(encode.len().saturating_sub(1)))
                .copied()
                .unwrap_or_default()
        };
        Some(format!(
            "audio: {} frames, encoding median {}, p99 {}, max {}, {} underruns, {} late ticks \
             (max {}), {} clock restarts",
            encode.len(),
            millis(percentile(50)),
            millis(percentile(99)),
            millis(encode.last().copied().unwrap_or_default()),
            metrics.underruns,
            metrics.late,
            millis(metrics.max_late),
            metrics.resyncs,
        ))
    }
}

fn millis(duration: Duration) -> String {
    format!("{:.2} ms", duration.as_secs_f64() * 1000.0)
}
