use tsclientlib::{Connection, OutCommandExt};

use super::{ClientError, ClientResult, ReplyTo};

/// What the rest of the bot asks the client to do on the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientRequest {
    SendMessage { to: ReplyTo, text: String },
    // TODO: `MoveTo(channel)`, `SetDescription(text)`, `SetNickname(text)`.
}

/// `Err` only when the command cannot leave the client, e.g. while tsclientlib reconnects.
/// A rejection by the server (missing permissions, flood protection) is not reported here.
// TODO: send with `send_with_result` and handle `StreamItem::MessageResult` in `Client::run`,
//   logging rejections at error, since they mean server permissions need fixing.
pub fn execute(connection: &mut Connection, request: ClientRequest) -> ClientResult<()> {
    match request {
        ClientRequest::SendMessage { to, text } => connection
            .get_state()
            .map_err(|source| ClientError::Request(Box::new(source)))?
            .send_message(to.into(), &text)
            .send(connection)
            .map_err(|source| ClientError::Request(Box::new(source))),
    }
}
