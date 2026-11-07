//! Audio pipeline, runs on a dedicated thread outside the async tasks:
//! input -> ffmpeg -> PCM f32 48 kHz stereo -> volume -> Opus -> bounded frame buffer.

mod decoder;
mod encoder;
