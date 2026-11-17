use tsclientlib::{ChannelId, Connection};

use super::{ClientError, ClientResult};
use crate::config::{ChannelRef, TeamspeakConfig};

/// Builds the connection from `[teamspeak]`.
/// Returns before the bot is on the server, the handshake goes on while `events()` is polled.
pub fn connect(config: &TeamspeakConfig) -> ClientResult<Connection> {
    let options = Connection::build(config.address.as_str()).name(config.nickname.clone());

    let options = match &config.home_channel {
        Some(ChannelRef::Id(id)) => options.channel_id(ChannelId(*id)),
        Some(ChannelRef::Name(path)) => options.channel(path.clone()),
        None => options,
    };

    // TODO: `.identity(...)` loaded from `state_dir/identity_file`, created and saved on first start,
    //   so the bot keeps its UID and server permissions between restarts.
    // TODO: `.password(...)` and `.channel_password(...)` from secrets.
    // TODO: the description is not a connect option, set it with a request after connecting.
    options
        .connect()
        .map_err(|source| ClientError::Connect(source.into()))
}
