use std::time::Duration;

use crate::teamspeak::metrics::{AudioMetrics, LATE};

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

#[test]
fn a_silent_interval_logs_nothing() {
    let mut metrics = AudioMetrics::default();
    metrics.tick(ms(1));
    assert_eq!(metrics.report(), None);
}

#[test]
fn the_report_sums_up_the_interval() {
    let mut metrics = AudioMetrics::default();
    for millis in 1..=100 {
        metrics.sent(Duration::from_micros(millis * 10));
    }
    metrics.tick(ms(1));
    metrics.tick(LATE + ms(1));
    metrics.tick(ms(30));
    metrics.underrun();
    metrics.resync();
    assert_eq!(
        metrics.report().unwrap(),
        "audio: 100 frames, encoding median 0.51 ms, p99 1.00 ms, max 1.00 ms, 1 underruns, \
         2 late ticks (max 30.00 ms), 1 clock restarts"
    );
}

#[test]
fn a_report_starts_counting_again() {
    let mut metrics = AudioMetrics::default();
    metrics.sent(ms(1));
    metrics.underrun();
    assert!(metrics.report().is_some());
    assert_eq!(metrics.report(), None);
}

#[test]
fn an_underrun_alone_is_reported() {
    let mut metrics = AudioMetrics::default();
    metrics.underrun();
    let report = metrics.report().unwrap();
    assert!(report.starts_with("audio: 0 frames"), "{report}");
    assert!(report.contains("1 underruns"), "{report}");
}
