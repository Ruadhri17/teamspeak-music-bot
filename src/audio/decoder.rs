//! Decoder: ffmpeg as a child process.
//!
//! Input always via stdin (`-i pipe:0`), never a URL in the arguments.
//! Output: `-f f32le -ac 2 -ar 48000 pipe:1`. Cancelling kills the process.
