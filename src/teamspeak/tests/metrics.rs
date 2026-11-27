use std::time::Duration;

use crate::teamspeak::metrics::{AudioMetrics, LATE, STALL};

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

fn playing() -> AudioMetrics {
    let mut metrics = AudioMetrics::default();
    metrics.sent(ms(1));
    metrics
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
        metrics.report().unwrap().line,
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
    let line = metrics.report().unwrap().line;
    assert!(line.starts_with("audio: 0 frames"), "{line}");
    assert!(line.contains("1 underruns"), "{line}");
}

#[test]
fn a_healthy_interval_is_no_trouble() {
    let mut metrics = playing();
    metrics.tick(ms(1));
    assert!(!metrics.report().unwrap().trouble);
}

#[test]
fn late_ticks_shorter_than_a_frame_are_no_trouble() {
    let mut metrics = playing();
    metrics.tick(LATE + ms(1));
    metrics.tick(STALL);
    let report = metrics.report().unwrap();
    assert!(report.line.contains("2 late ticks"), "{}", report.line);
    assert!(!report.trouble);
}

#[test]
fn a_tick_later_than_a_frame_is_trouble() {
    let mut metrics = playing();
    metrics.tick(STALL + ms(1));
    assert!(metrics.report().unwrap().trouble);
}

#[test]
fn an_underrun_is_trouble() {
    let mut metrics = playing();
    metrics.underrun();
    assert!(metrics.report().unwrap().trouble);
}

#[test]
fn a_clock_restart_is_trouble() {
    let mut metrics = playing();
    metrics.resync();
    assert!(metrics.report().unwrap().trouble);
}

#[test]
fn trouble_does_not_carry_over_to_the_next_interval() {
    let mut metrics = playing();
    metrics.underrun();
    assert!(metrics.report().unwrap().trouble);
    metrics.sent(ms(1));
    assert!(!metrics.report().unwrap().trouble);
}
