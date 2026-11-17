//! Decoder: ffmpeg as a child process.
//!
//! Input always via stdin (`-i pipe:0`), never a URL in the arguments.
//! Output: `-f f32le -ac 2 -ar 48000 pipe:1`. Cancelling kills the process.

use std::{
    io::Read,
    path::Path,
    process::{Child, ChildStdout, Command, Stdio},
};

use super::{AudioInput, FRAME_SAMPLES};

/// One frame as raw `f32le` bytes from ffmpeg.
const FRAME_BYTES: usize = FRAME_SAMPLES * size_of::<f32>();

pub struct Decoder {
    child: Child,
    stdout: ChildStdout,
}

impl Decoder {
    pub fn spawn(ffmpeg_path: &Path, mut input: AudioInput) -> std::io::Result<Self> {
        let mut child = Command::new(ffmpeg_path)
            .args([
                "-nostdin",
                "-loglevel",
                "error",
                "-i",
                "pipe:0",
                "-f",
                "f32le",
                "-ac",
                "2",
                "-ar",
                "48000",
                "pipe:1",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        // Both pipes are set to `piped()` above, so `take` returns `Some`.
        let mut stdin = child.stdin.take().unwrap();
        // Written on a separate thread, otherwise the stdin and stdout pipes can deadlock.
        let _writer = std::thread::spawn(move || {
            // `BrokenPipe` is normal when ffmpeg is killed early (`!song next`).
            let _ = std::io::copy(&mut input, &mut stdin);
            // `stdin` is dropped here, ffmpeg sees end of input.
        });

        let stdout = child.stdout.take().unwrap();

        Ok(Self { child, stdout })
    }

    /// Next 20 ms of audio, `None` at the end of the track.
    pub fn read_frame(&mut self) -> Option<[f32; FRAME_SAMPLES]> {
        let mut bytes = [0u8; FRAME_BYTES];
        self.stdout.read_exact(&mut bytes).ok()?;

        let mut frame = [0f32; FRAME_SAMPLES];
        for (sample, chunk) in frame.iter_mut().zip(bytes.chunks_exact(4)) {
            // `chunks_exact(4)` gives 4-byte slices, so the conversion to `[u8; 4]` cannot fail.
            *sample = f32::from_le_bytes(chunk.try_into().unwrap());
        }
        Some(frame)
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // Kills ffmpeg if it is still running and reaps it to avoid a zombie process.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
