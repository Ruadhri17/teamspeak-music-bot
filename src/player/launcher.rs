//! Starting the audio of a track. Defined as a trait so tests can substitute a launcher
//! that starts nothing.

use std::path::PathBuf;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    audio::{self, AudioInput, Frame},
    config::{AudioConfig, Config},
};

/// Starts the playback of one track.
/// `Sync` because the spawned player holds `&self` across `.await` in `reply`.
pub trait Launcher: Send + Sync {
    /// `on_done` is boxed because methods called through `dyn Launcher` cannot be generic.
    fn launch(
        &mut self,
        input: AudioInput,
        stream: u64,
        stop: CancellationToken,
        on_done: Box<dyn FnOnce() + Send>,
    );
}

/// Production launcher. Runs the audio thread with ffmpeg and Opus (`audio::play`).
pub struct AudioLauncher {
    ffmpeg_path: PathBuf,
    audio: AudioConfig,
    frames: mpsc::Sender<Frame>,
}

impl AudioLauncher {
    pub fn new(config: &Config, frames: mpsc::Sender<Frame>) -> Self {
        Self {
            ffmpeg_path: config.paths.ffmpeg_path.clone(),
            audio: config.audio.clone(),
            frames,
        }
    }
}

impl Launcher for AudioLauncher {
    fn launch(
        &mut self,
        input: AudioInput,
        stream: u64,
        stop: CancellationToken,
        on_done: Box<dyn FnOnce() + Send>,
    ) {
        audio::play(
            self.ffmpeg_path.clone(),
            input,
            &self.audio,
            stream,
            self.frames.clone(),
            stop,
            on_done,
        );
    }
}
