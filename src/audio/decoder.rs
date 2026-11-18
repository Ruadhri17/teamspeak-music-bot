//! Decoder: ffmpeg as a child process.
//!
//! Input always via stdin (`-i pipe:0`), never a URL in the arguments.
//! Output: `-f f32le -ac 2 -ar 48000 pipe:1`. Cancelling kills the process.

use std::{
    io::{self, Read},
    path::Path,
    process::{Child, ChildStdout, Command, Stdio},
    thread::JoinHandle,
};

use super::{AudioError, AudioInput, FRAME_SAMPLES};

/// One frame as raw `f32le` bytes from ffmpeg.
const FRAME_BYTES: usize = FRAME_SAMPLES * size_of::<f32>();
/// How much of the ffmpeg error output is kept for the log.
const STDERR_TAIL: usize = 2048;

pub struct Decoder {
    child: Child,
    stdout: ChildStdout,
    /// Copies the input to ffmpeg, the result tells a read error from the end of the input.
    writer: Option<JoinHandle<io::Result<()>>>,
    /// Last `STDERR_TAIL` bytes of the ffmpeg error output.
    stderr: Option<JoinHandle<String>>,
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
            .stderr(Stdio::piped())
            .spawn()?;

        // All pipes are set to `piped()` above, so `take` returns `Some`.
        let mut stdin = child.stdin.take().unwrap();
        // Written on a separate thread, otherwise the stdin and stdout pipes can deadlock.
        let writer = std::thread::spawn(move || {
            match io::copy(&mut input, &mut stdin) {
                // Normal when ffmpeg exits early: killed (`!song next`) or failed on bad input.
                Err(err) if err.kind() == io::ErrorKind::BrokenPipe => Ok(()),
                result => result.map(drop),
            }
            // `stdin` is dropped here, ffmpeg sees end of input.
        });

        // Read on its own thread, a full stderr pipe would block ffmpeg.
        let mut stderr = child.stderr.take().unwrap();
        let stderr = std::thread::spawn(move || read_tail(&mut stderr));

        let stdout = child.stdout.take().unwrap();

        Ok(Self {
            child,
            stdout,
            writer: Some(writer),
            stderr: Some(stderr),
        })
    }

    /// Next 20 ms of audio, `None` at the end of the output.
    /// A last incomplete frame (under 20 ms) is dropped.
    pub fn read_frame(&mut self) -> io::Result<Option<[f32; FRAME_SAMPLES]>> {
        let mut bytes = [0u8; FRAME_BYTES];
        match self.stdout.read_exact(&mut bytes) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(err) => return Err(err),
        }

        let mut frame = [0f32; FRAME_SAMPLES];
        for (sample, chunk) in frame.iter_mut().zip(bytes.chunks_exact(4)) {
            // `chunks_exact(4)` gives 4-byte slices, so the conversion to `[u8; 4]` cannot fail.
            *sample = f32::from_le_bytes(chunk.try_into().unwrap());
        }
        Ok(Some(frame))
    }

    /// Call after `read_frame` returned `None`. Tells the end of the track from a failure:
    /// an input read error or a non-zero ffmpeg exit (bad data, unsupported format).
    pub fn finish(&mut self) -> Result<(), AudioError> {
        let status = self.child.wait().map_err(AudioError::Output)?;
        if let Some(writer) = self.writer.take() {
            writer
                .join()
                .unwrap_or_else(|_| Err(io::Error::other("input thread panicked")))
                .map_err(AudioError::Input)?;
        }
        if !status.success() {
            let stderr = self
                .stderr
                .take()
                .and_then(|handle| handle.join().ok())
                .unwrap_or_default();
            return Err(AudioError::Ffmpeg { status, stderr });
        }
        Ok(())
    }
}

/// Reads `reader` to the end, keeping the last `STDERR_TAIL` bytes.
fn read_tail(reader: &mut impl Read) -> String {
    let mut tail = Vec::new();
    let mut buf = [0u8; 4096];
    while let Ok(n @ 1..) = reader.read(&mut buf) {
        tail.extend_from_slice(&buf[..n]);
        if tail.len() > STDERR_TAIL {
            tail.drain(..tail.len() - STDERR_TAIL);
        }
    }
    String::from_utf8_lossy(&tail).trim().to_string()
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // Kills ffmpeg if it is still running and reaps it to avoid a zombie process.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
