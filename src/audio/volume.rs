//! Volume control: maps a volume percent to a gain factor and applies it to PCM frames,
//! with a linear ramp when the factor changes.

use super::{CHANNELS, FRAME_SAMPLES, SAMPLES_PER_CHANNEL};
use crate::config::Volume;

/// The ear hears loudness on a log scale, so the factor grows exponentially with the percent
/// (after dr-lex.be "volume controls", 40 dB between the ends of the exponential part).
/// 100 % keeps the track as it is, 50 % is about -21 dB, 10 % about -45 dB, 0 % is silence.
pub fn gain(volume: Volume) -> f32 {
    const A: f32 = 1e-2;
    const B: f32 = 4.615_12;
    match volume.get() {
        0 => 0.0,
        Volume::MAX => 1.0,
        percent => {
            let x = f32::from(percent) / f32::from(Volume::MAX);
            (A * (B * x).exp() - A).clamp(0.0, 1.0)
        }
    }
}

/// Multiplies the frame by a factor that goes in a straight line from `from` to `to`.
/// A jump in the middle of a wave is a click, a ramp over 20 ms is not heard.
/// Both channels of a sample get the same factor. The factor is at most 1, so no clipping.
pub fn apply(frame: &mut [f32; FRAME_SAMPLES], from: f32, to: f32) {
    if from == to {
        if to != 1.0 {
            frame.iter_mut().for_each(|sample| *sample *= to);
        }
        return;
    }
    let step = (to - from) / SAMPLES_PER_CHANNEL as f32;
    let (pairs, _) = frame.as_chunks_mut::<CHANNELS>();
    for (index, pair) in pairs.iter_mut().enumerate() {
        let factor = from + step * (index + 1) as f32;
        pair.iter_mut().for_each(|sample| *sample *= factor);
    }
}
