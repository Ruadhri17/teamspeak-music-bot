//! Outgoing audio stage: applies the volume to a PCM frame and encodes it into one Opus packet.
//! Used by the client for every frame it sends.

use super::{FRAME_SAMPLES, encoder::OpusEncoder, volume};
use crate::config::{Bitrate, Complexity, Volume};

/// Volume and Opus encoder state for the whole connection. Consecutive tracks share one
/// encoder, a volume change takes effect from the next frame.
pub struct Output {
    encoder: OpusEncoder,
    /// The factor at the end of the last frame, where the next ramp starts.
    gain: f32,
}

impl Output {
    pub fn new(bitrate: Bitrate, complexity: Complexity) -> audiopus::Result<Self> {
        Ok(Self {
            encoder: OpusEncoder::new(bitrate, complexity)?,
            gain: 1.0,
        })
    }

    /// Applies `volume` (with a ramp from the previous one) and encodes one Opus packet.
    pub fn encode(
        &mut self,
        pcm: &mut [f32; FRAME_SAMPLES],
        volume: Volume,
    ) -> audiopus::Result<Vec<u8>> {
        let target = volume::gain(volume);
        volume::apply(pcm, self.gain, target);
        self.gain = target;
        self.encoder.encode(pcm)
    }

    /// Call after the end-of-speech packet: listeners start a fresh decoder for the next
    /// packet, so the encoder starts fresh too.
    pub fn reset(&mut self) -> audiopus::Result<()> {
        self.encoder.reset()
    }
}
