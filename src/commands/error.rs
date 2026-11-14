/// Shown to the user on the chat as it is, so every message is short and says how to fix it.
/// No `#[source]`, internal details are not the user's business.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    /// Known prefix (`!song`, `!queue`, `!bot`), unknown or malformed command after it.
    #[error("unknown command, try: {0}")]
    UnknownCommand(&'static str),

    /// Known command, wrong arguments.
    #[error("usage: {0}")]
    Usage(&'static str),

    #[error("unknown flag {0}, try: --source, --track, --album, --playlist")]
    UnknownFlag(String),

    #[error("{0} needs a value")]
    MissingValue(&'static str),

    #[error("{0} is given more than once")]
    RepeatedFlag(&'static str),

    #[error("use only one of --track, --album, --playlist")]
    ConflictingKinds,

    #[error("unknown source {0}, try: tidal, spotify, youtube, soundcloud, local")]
    UnknownSource(String),

    #[error("{0} is not a positive number")]
    InvalidNumber(String),
}
