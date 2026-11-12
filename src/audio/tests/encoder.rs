use std::f32::consts::TAU;

use audiopus::{Channels, SampleRate, coder::Decoder};

use crate::{
    audio::{FRAME_SAMPLES, SAMPLE_RATE, SAMPLES_PER_CHANNEL, encoder::OpusEncoder},
    config::{Bitrate, Complexity},
};

fn encoder(kbps: u32) -> OpusEncoder {
    OpusEncoder::new(Bitrate::new(kbps).unwrap(), Complexity::new(10).unwrap()).unwrap()
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

fn encode_second(encoder: &mut OpusEncoder) -> Vec<Vec<u8>> {
    (0..50)
        .map(|i| encoder.encode(&sine_frame(i)).unwrap())
        .collect()
}

#[test]
fn packets_are_non_empty_and_within_opus_limit() {
    let packets = encode_second(&mut encoder(96));
    assert!(packets.iter().all(|p| !p.is_empty() && p.len() <= 1276));
}

#[test]
fn packets_fit_the_configured_bitrate() {
    let total: usize = encode_second(&mut encoder(96)).iter().map(Vec::len).sum();
    let kbps = total * 8 / 1000;
    assert!((70..=120).contains(&kbps), "{kbps} kb/s");
}

#[test]
fn higher_bitrate_gives_bigger_packets() {
    let low: usize = encode_second(&mut encoder(32)).iter().map(Vec::len).sum();
    let high: usize = encode_second(&mut encoder(192)).iter().map(Vec::len).sum();
    assert!(high > low * 2, "low {low} B, high {high} B");
}

#[test]
fn packets_decode_back_to_sound() {
    let mut encoder = encoder(96);
    let mut decoder = Decoder::new(SampleRate::Hz48000, Channels::Stereo).unwrap();
    let mut pcm = [0.0f32; FRAME_SAMPLES];
    let mut loudest = 0.0f32;
    for i in 0..50 {
        let packet = encoder.encode(&sine_frame(i)).unwrap();
        let packet = (&packet[..]).try_into().unwrap();
        let output = (&mut pcm[..]).try_into().unwrap();
        let samples = decoder.decode_float(Some(packet), output, false).unwrap();
        assert_eq!(samples, SAMPLES_PER_CHANNEL);
        loudest = pcm.iter().fold(loudest, |max, s| max.max(s.abs()));
    }
    assert!((0.3..=0.7).contains(&loudest), "loudest sample {loudest}");
}
