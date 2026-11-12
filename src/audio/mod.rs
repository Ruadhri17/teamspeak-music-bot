//! Audio pipeline, runs on a dedicated thread outside the async tasks:
//! input -> ffmpeg -> PCM f32 48 kHz stereo -> volume -> Opus -> bounded frame buffer.

mod decoder;
mod encoder;

use std::{io::Read, path::PathBuf, thread::JoinHandle, time::Duration};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::config::AudioConfig;
use decoder::Decoder;
use encoder::OpusEncoder;

pub const SAMPLE_RATE: usize = 48_000;
pub const CHANNELS: usize = 2;
/// Length of one Opus frame, also the client's sending interval.
pub const FRAME_DURATION: Duration = Duration::from_millis(20);
/// 960 samples per channel in 20 ms.
pub const SAMPLES_PER_CHANNEL: usize = SAMPLE_RATE * FRAME_DURATION.as_millis() as usize / 1000;
/// One frame, interleaved (left, right, left, ...).
pub const FRAME_SAMPLES: usize = SAMPLES_PER_CHANNEL * CHANNELS;

/// Audio data from any source
pub type AudioInput = Box<dyn Read + Send>;

/// Plays `input` on a new thread, pushing Opus packets into `frames` until the track ends,
/// `stop` is cancelled or the receiver is gone.
pub fn play(
    ffmpeg_path: PathBuf,
    input: AudioInput,
    audio: &AudioConfig,
    frames: mpsc::Sender<Vec<u8>>,
    stop: CancellationToken,
) -> JoinHandle<()> {
    let (bitrate, complexity) = (audio.bitrate, audio.complexity);

    std::thread::spawn(move || {
        let mut decoder = Decoder::spawn(&ffmpeg_path, input).unwrap();
        let mut encoder = OpusEncoder::new(bitrate, complexity).unwrap();
        while let Some(frame) = decoder.read_frame() {
            if stop.is_cancelled() {
                break;
            }
            let packet = encoder.encode(&frame).unwrap();
            // Waits while the buffer is full, `Err` means the client is gone.
            if frames.blocking_send(packet).is_err() {
                break;
            }
        }
    })
}

#[cfg(test)]
mod tests;
