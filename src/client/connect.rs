use secrecy::ExposeSecret;
use tsclientlib::{ChannelId, Connection, Identity};

use super::{ClientError, ClientResult};
use crate::{
    config::{ChannelRef, TeamspeakConfig},
    secrets::TeamspeakSecrets,
};

/// Builds the connection from `[teamspeak]` and the secrets.
/// Returns before the bot is on the server, the handshake goes on while `events()` is polled.
pub fn connect(
    config: &TeamspeakConfig,
    secrets: &TeamspeakSecrets,
    identity: Identity,
) -> ClientResult<Connection> {
    let options = Connection::build(config.address.as_str())
        .name(config.nickname.clone())
        .identity(identity);

    let options = match &config.home_channel {
        Some(ChannelRef::Id(id)) => options.channel_id(ChannelId(*id)),
        Some(ChannelRef::Name(path)) => options.channel(path.clone()),
        None => options,
    };

    // tsclientlib keeps its own copies of the passwords as plain strings.
    let options = match &secrets.server_password {
        Some(password) => options.password(password.expose_secret().to_owned()),
        None => options,
    };
    let options = match &secrets.channel_password {
        Some(password) => options.channel_password(password.expose_secret().to_owned()),
        None => options,
    };

    // TODO: the description is not a connect option, set it with a request after connecting.
    options
        .connect()
        .map_err(|source| ClientError::Connect(source.into()))
}
