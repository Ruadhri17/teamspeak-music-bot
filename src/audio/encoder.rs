//! Opus encoder (audiopus): stereo, 48 kHz, `Application::Audio`, 20 ms frames (960 samples per channel),
//! bitrate and complexity from the config. Sent as `CodecType::OpusMusic`.

use audiopus::{Application, Channels, SampleRate, coder::Encoder};

use super::FRAME_SAMPLES;
use crate::config::{Bitrate, Complexity};

/// Output buffer size recommended by the libopus docs for `opus_encode`.
const MAX_PACKET: usize = 4000;

pub struct OpusEncoder {
    encoder: Encoder,
    output: [u8; MAX_PACKET],
}

impl OpusEncoder {
    /// Create one per track, Opus state carries over between frames.
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
}
