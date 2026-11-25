use crate::secrets::identity::IdentityError;

/// `tsclientlib::Error` is large, so it is boxed to keep every `TeamspeakResult` small.
#[derive(Debug, thiserror::Error)]
pub enum TeamspeakError {
    #[error("cannot connect to the TeamSpeak server")]
    Connect(#[source] Box<tsclientlib::Error>),

    #[error("connection to the TeamSpeak server failed")]
    Connection(#[source] Box<tsclientlib::Error>),

    /// Kicked, banned, or the connection ended in another way tsclientlib does not retry.
    #[error("the TeamSpeak server closed the connection ({0})")]
    Closed(String),

    #[error("cannot send a request to the TeamSpeak server")]
    Request(#[source] Box<tsclientlib::Error>),

    #[error(transparent)]
    Identity(#[from] IdentityError),

    #[error("cannot create the Opus encoder")]
    Encoder(#[source] audiopus::Error),
}

pub type TeamspeakResult<T> = Result<T, TeamspeakError>;
