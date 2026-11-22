use crate::chat;

/// Shown on the chat as it is, so every message is short and says how to fix it.
/// Words the user typed go through `chat::escape`, so they cannot format the reply.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    /// Known prefix (`!song`, `!queue`, `!bot`), unknown or malformed command after it.
    #[error("unknown command, try: {0}")]
    UnknownCommand(&'static str),

    /// Unknown prefix one typo away from a known one, e.g. `!sond`.
    #[error("unknown command, did you mean {0}?")]
    DidYouMean(&'static str),

    /// Known command, wrong arguments.
    #[error("usage: {0}")]
    Usage(&'static str),

    #[error("unknown flag {}, try: --source, --track, --album, --playlist", chat::escape(.0))]
    UnknownFlag(String),

    #[error("{0} needs a value")]
    MissingValue(&'static str),

    #[error("{0} is given more than once")]
    RepeatedFlag(&'static str),

    #[error("use only one of --track, --album, --playlist")]
    ConflictingKinds,

    #[error("unknown source {}, try: tidal, spotify, youtube, soundcloud, local", chat::escape(.0))]
    UnknownSource(String),

    #[error("{} is not a positive number", chat::escape(.0))]
    InvalidNumber(String),
}
