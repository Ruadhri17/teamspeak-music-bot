use crate::secrets::identity::IdentityError;

/// `tsclientlib::Error` is large, so it is boxed to keep every `ClientResult` small.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("cannot connect to the TeamSpeak server")]
    Connect(#[source] Box<tsclientlib::Error>),

    #[error("connection to the TeamSpeak server failed")]
    Connection(#[source] Box<tsclientlib::Error>),

    #[error("cannot send a request to the TeamSpeak server")]
    Request(#[source] Box<tsclientlib::Error>),

    #[error(transparent)]
    Identity(#[from] IdentityError),

    #[error("cannot create the Opus encoder")]
    Encoder(#[source] audiopus::Error),
}

pub type ClientResult<T> = Result<T, ClientError>;
