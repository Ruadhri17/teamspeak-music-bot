//! Outgoing audio stage: encodes a PCM frame into one Opus packet.
//! Used by the client for every frame it sends.

use super::{FRAME_SAMPLES, encoder::OpusEncoder};
use crate::config::{Bitrate, Complexity};

/// Opus encoder state for the whole connection. Consecutive tracks share one encoder.
pub struct Output {
    encoder: OpusEncoder,
}

impl Output {
    pub fn new(bitrate: Bitrate, complexity: Complexity) -> audiopus::Result<Self> {
        Ok(Self {
            encoder: OpusEncoder::new(bitrate, complexity)?,
        })
    }

    /// Encodes one Opus packet.
    pub fn encode(&mut self, pcm: &[f32; FRAME_SAMPLES]) -> audiopus::Result<Vec<u8>> {
        self.encoder.encode(pcm)
    }

    /// Call after the end-of-speech packet: listeners start a fresh decoder for the next
    /// packet, so the encoder starts fresh too.
    pub fn reset(&mut self) -> audiopus::Result<()> {
        self.encoder.reset()
    }
}
