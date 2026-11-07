//! Bot presence in a channel, driven by `[events]` (`on_party`, `on_idle`, `on_alone`).
//!
//! Welcome sound after joining, idle and alone timers, goodbye sound,
//! returning to the home channel and clearing the queue. Guards against a double goodbye
//! and late events.
