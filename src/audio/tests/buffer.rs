use std::time::Duration;

use crate::audio::{FRAME_DURATION, MAX_BUFFER, frames_in};

#[test]
fn the_default_buffer_holds_250_frames() {
    assert_eq!(frames_in(Duration::from_secs(5)), 250);
}

#[test]
fn a_partial_frame_counts_as_a_whole_one() {
    assert_eq!(frames_in(FRAME_DURATION), 1);
    assert_eq!(frames_in(Duration::from_millis(21)), 2);
    assert_eq!(frames_in(Duration::from_millis(1)), 1);
}

#[test]
fn even_the_largest_buffer_fits_a_channel() {
    assert_eq!(frames_in(MAX_BUFFER), 3000);
}
