use std::f32::consts::TAU;

use audiopus::{Channels, SampleRate, coder::Decoder};

use crate::{
    audio::{FRAME_SAMPLES, Output, SAMPLE_RATE, SAMPLES_PER_CHANNEL, volume::gain},
    config::{Bitrate, Complexity, Volume},
};

fn output() -> Output {
    Output::new(Bitrate::new(96).unwrap(), Complexity::new(10).unwrap()).unwrap()
}

fn sine_frame(index: usize) -> [f32; FRAME_SAMPLES] {
    let mut frame = [0.0; FRAME_SAMPLES];
    for (i, pair) in frame.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        let t = (index * SAMPLES_PER_CHANNEL + i) as f32 / SAMPLE_RATE as f32;
        let sample = 0.5 * (TAU * 440.0 * t).sin();
        pair[0] = sample;
        pair[1] = sample;
    }
    frame
}

fn loudness_of_last_frames(output: &mut Output, volume: Volume) -> f32 {
    let mut decoder = Decoder::new(SampleRate::Hz48000, Channels::Stereo).unwrap();
    let mut pcm = [0.0f32; FRAME_SAMPLES];
    let mut sum = 0.0f32;
    let mut count = 0;
    for i in 0..50 {
        let packet = output.encode(&mut sine_frame(i), volume).unwrap();
        let packet = (&packet[..]).try_into().unwrap();
        let out = (&mut pcm[..]).try_into().unwrap();
        decoder.decode_float(Some(packet), out, false).unwrap();
        if i >= 25 {
            sum += pcm.iter().map(|s| s * s).sum::<f32>();
            count += pcm.len();
        }
    }
    (sum / count as f32).sqrt()
}

#[test]
fn volume_makes_the_decoded_sound_quieter_by_its_gain() {
    let full = loudness_of_last_frames(&mut output(), Volume::FULL);
    let quiet = loudness_of_last_frames(&mut output(), Volume::new(50).unwrap());
    let ratio = quiet / full;
    let expected = gain(Volume::new(50).unwrap());
    assert!(
        (ratio - expected).abs() < expected * 0.2,
        "ratio {ratio}, expected {expected}"
    );
}

#[test]
fn zero_volume_is_silence() {
    let silent = loudness_of_last_frames(&mut output(), Volume::new(0).unwrap());
    assert!(silent < 1e-3, "{silent}");
}

#[test]
fn new_volume_applies_from_the_next_frame() {
    let mut output = output();
    let mut first = sine_frame(0);
    output.encode(&mut first, Volume::FULL).unwrap();
    let mut second = sine_frame(1);
    output.encode(&mut second, Volume::new(0).unwrap()).unwrap();
    assert_eq!(second[FRAME_SAMPLES - 1], 0.0);
    let mut third = sine_frame(2);
    output.encode(&mut third, Volume::new(0).unwrap()).unwrap();
    assert!(third.iter().all(|&s| s == 0.0));
}

#[test]
fn reset_keeps_the_output_working() {
    let mut output = output();
    output.encode(&mut sine_frame(0), Volume::FULL).unwrap();
    output.reset().unwrap();
    let packet = output.encode(&mut sine_frame(1), Volume::FULL).unwrap();
    assert!(!packet.is_empty());
}
