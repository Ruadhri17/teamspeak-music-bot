//! Chat command parser: `!song`, `!queue`, `!bot`.
//!
//! Turns message text into a `Command` with flags (`--source`, `--track|--album|--playlist`)
//! and returns readable usage errors. Does not execute commands, that is the player's job.
