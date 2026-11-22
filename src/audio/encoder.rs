//! Opus encoder (audiopus): stereo, 48 kHz, `Application::Audio`, 20 ms frames (960 samples per channel),
//! bitrate and complexity from the config. Sent as `CodecType::OpusMusic`.

use audiopus::{
    Application, Channels, SampleRate,
    coder::{Encoder, GenericCtl},
};

use super::FRAME_SAMPLES;
use crate::config::{Bitrate, Complexity};

/// Largest Opus packet TeamSpeak 3 accepts: 500 B per voice packet minus the 16 B header.
/// tsproto neither splits nor checks voice packets. Opus treats the buffer size as a hard
/// limit and lowers the quality of a frame that would not fit (measured: 192 kb/s gives up to 754 B).
pub const MAX_PACKET: usize = 484;

pub struct OpusEncoder {
    encoder: Encoder,
    output: [u8; MAX_PACKET],
}

impl OpusEncoder {
    /// Opus state carries over between frames, `reset` starts it fresh.
    pub fn new(bitrate: Bitrate, complexity: Complexity) -> audiopus::Result<Self> {
        let mut encoder = Encoder::new(SampleRate::Hz48000, Channels::Stereo, Application::Audio)?;
        // The config is in kb/s, Opus wants b/s. `Bitrate::MAX` (192) * 1000 fits in `i32`.
        encoder.set_bitrate(audiopus::Bitrate::BitsPerSecond(
            bitrate.get() as i32 * 1000,
        ))?;
        encoder.set_complexity(complexity.get())?;
        Ok(Self {
            encoder,
            output: [0; MAX_PACKET],
        })
    }

    /// Encodes one 20 ms frame into one Opus packet of variable length.
    pub fn encode(&mut self, frame: &[f32; FRAME_SAMPLES]) -> audiopus::Result<Vec<u8>> {
        let len = self.encoder.encode_float(frame, &mut self.output)?;
        Ok(self.output[..len].to_vec())
    }

    /// Back to the state of a new encoder.
    pub fn reset(&mut self) -> audiopus::Result<()> {
        self.encoder.reset_state()
    }
}
