use crate::{
    audio::{
        FRAME_SAMPLES,
        volume::{apply, gain},
    },
    config::Volume,
};

fn volume(percent: u8) -> Volume {
    Volume::new(percent).unwrap()
}

fn ones() -> [f32; FRAME_SAMPLES] {
    [1.0; FRAME_SAMPLES]
}

#[test]
fn ends_of_the_scale_are_silence_and_the_original() {
    assert_eq!(gain(volume(0)), 0.0);
    assert_eq!(gain(volume(100)), 1.0);
}

#[test]
fn gain_only_goes_up() {
    let gains: Vec<f32> = (0..=100).map(|p| gain(volume(p))).collect();
    assert!(gains.windows(2).all(|pair| pair[0] < pair[1]), "{gains:?}");
}

#[test]
fn half_volume_is_about_minus_21_db() {
    let db = 20.0 * gain(volume(50)).log10();
    assert!((-22.0..=-20.0).contains(&db), "{db} dB");
}

#[test]
fn ten_percent_is_about_minus_45_db() {
    let db = 20.0 * gain(volume(10)).log10();
    assert!((-46.0..=-43.0).contains(&db), "{db} dB");
}

#[test]
fn lowest_step_is_quiet_but_not_silent() {
    let one = gain(volume(1));
    assert!(one > 0.0 && one < 0.001, "{one}");
}

#[test]
fn full_volume_leaves_the_frame_alone() {
    let mut frame = ones();
    apply(&mut frame, 1.0, 1.0);
    assert_eq!(frame, ones());
}

#[test]
fn steady_factor_scales_every_sample() {
    let mut frame = ones();
    apply(&mut frame, 0.25, 0.25);
    assert!(frame.iter().all(|&s| s == 0.25));
    apply(&mut frame, 0.0, 0.0);
    assert!(frame.iter().all(|&s| s == 0.0));
}

#[test]
fn change_is_a_ramp_without_jumps() {
    let mut frame = ones();
    apply(&mut frame, 1.0, 0.0);
    let left: Vec<f32> = frame.iter().step_by(2).copied().collect();
    let right: Vec<f32> = frame.iter().skip(1).step_by(2).copied().collect();
    assert_eq!(left, right);
    assert!(left[0] < 1.0 && left[0] > 0.99, "{}", left[0]);
    assert_eq!(*left.last().unwrap(), 0.0);
    assert!(left.windows(2).all(|pair| pair[1] < pair[0]));
    let biggest_step = left
        .windows(2)
        .map(|pair| pair[0] - pair[1])
        .fold(0.0f32, f32::max);
    assert!(biggest_step < 0.002, "{biggest_step}");
}

#[test]
fn ramp_up_ends_at_the_new_factor() {
    let mut frame = ones();
    apply(&mut frame, 0.0, 0.5);
    assert!(frame[0] > 0.0 && frame[0] < 0.001);
    assert!((frame[FRAME_SAMPLES - 1] - 0.5).abs() < 1e-6);
}
