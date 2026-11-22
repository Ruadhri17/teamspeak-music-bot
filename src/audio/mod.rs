//! Audio pipeline: input -> ffmpeg -> PCM f32 48 kHz stereo -> bounded frame buffer
//! -> volume -> Opus -> send. The audio thread fills the buffer, the client takes
//! one frame every 20 ms.
//!
//! The buffer holds PCM, so processing after it (volume) applies from the next frame.
//!
//! - `decoder`: ffmpeg as a child process, PCM frames from its stdout
//! - `encoder`: Opus encoder with the TeamSpeak packet limit
//! - `output`: volume and encoding of each outgoing frame
//! - `volume`: gain curve and ramp

mod decoder;
mod encoder;
mod output;
pub mod volume;

use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    process::ExitStatus,
    thread::JoinHandle,
    time::Duration,
};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub(crate) use decoder::{Decoder, read_tail};
pub use output::Output;

pub const SAMPLE_RATE: usize = 48_000;
pub const CHANNELS: usize = 2;
/// Length of one Opus frame, also the client's sending interval.
pub const FRAME_DURATION: Duration = Duration::from_millis(20);
/// 960 samples per channel in 20 ms.
pub const SAMPLES_PER_CHANNEL: usize = SAMPLE_RATE * FRAME_DURATION.as_millis() as usize / 1000;
/// One frame, interleaved (left, right, left, ...).
pub const FRAME_SAMPLES: usize = SAMPLES_PER_CHANNEL * CHANNELS;

/// Audio data from any source, always given to ffmpeg on its stdin, never as a path or URL.
pub enum AudioInput {
    /// A seekable file, becomes ffmpeg's stdin directly. Seeking is needed by formats
    /// with the index at the end, e.g. MP4/M4A with `moov` last, which fail through a pipe.
    File(File),
    /// Data that cannot seek (network, another process), copied into a pipe on a thread.
    Stream(Box<dyn Read + Send>),
}

/// Why a track stopped before its end. A stop on request (`stop`, client gone) is not an error.
/// Only for logs, the ffmpeg output may contain details of the input.
#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("cannot start ffmpeg: {0}")]
    Spawn(#[source] io::Error),

    #[error("cannot read the track: {0}")]
    Input(#[source] io::Error),

    #[error("cannot read the ffmpeg output: {0}")]
    Output(#[source] io::Error),

    #[error("ffmpeg failed ({status}): {stderr}")]
    Ffmpeg { status: ExitStatus, stderr: String },

    #[error("the audio thread panicked")]
    Panicked,
}

pub type PlayResult = Result<(), AudioError>;

/// 20 ms of PCM and the number of the playback it belongs to.
#[derive(Debug)]
pub struct Frame {
    pub stream: u64,
    /// Boxed, so the 7.7 KB of samples is not copied on every move through the channel.
    pub pcm: Box<[f32; FRAME_SAMPLES]>,
}

/// Plays `input` on a new thread, pushing PCM frames tagged with `stream` into `frames`
/// until the track ends, `stop` is cancelled or the receiver is gone.
/// Then the thread calls `on_done` with the result, also when it panics.
pub fn play(
    ffmpeg_path: PathBuf,
    input: AudioInput,
    stream: u64,
    frames: mpsc::Sender<Frame>,
    stop: CancellationToken,
    on_done: impl FnOnce(PlayResult) + Send + 'static,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let done = DoneGuard(Some(on_done));
        let result = decode(&ffmpeg_path, input, stream, &frames, &stop);
        // A stopped track fails to read once ffmpeg is killed, that is not an error.
        done.finish(if stop.is_cancelled() { Ok(()) } else { result });
    })
}

fn decode(
    ffmpeg_path: &Path,
    input: AudioInput,
    stream: u64,
    frames: &mpsc::Sender<Frame>,
    stop: &CancellationToken,
) -> PlayResult {
    let mut decoder = Decoder::spawn(ffmpeg_path, input).map_err(AudioError::Spawn)?;
    while let Some(pcm) = decoder.read_frame().map_err(AudioError::Output)? {
        if stop.is_cancelled() {
            return Ok(());
        }
        let frame = Frame {
            stream,
            pcm: Box::new(pcm),
        };
        // Blocks while the buffer is full. `Err` means the client is gone.
        if frames.blocking_send(frame).is_err() {
            return Ok(());
        }
    }
    decoder.finish()
}

/// Calls `on_done` exactly once. When the thread panics, `Drop` reports `Panicked`,
/// otherwise the player would stay `Playing` forever.
struct DoneGuard<F: FnOnce(PlayResult)>(Option<F>);

impl<F: FnOnce(PlayResult)> DoneGuard<F> {
    fn finish(mut self, result: PlayResult) {
        if let Some(on_done) = self.0.take() {
            on_done(result);
        }
    }
}

impl<F: FnOnce(PlayResult)> Drop for DoneGuard<F> {
    fn drop(&mut self) {
        if let Some(on_done) = self.0.take() {
            on_done(Err(AudioError::Panicked));
        }
    }
}

#[cfg(test)]
mod tests;
