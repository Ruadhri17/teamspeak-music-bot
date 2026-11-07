//! Entry point: CLI (`init`, `run`, `tidal-login`), loading config and secrets,
//! starting the tasks (TS client, player, presence) and wiring them with channels.

mod audio;
mod client;
mod commands;
mod config;
mod permissions;
mod player;
mod presence;
mod secrets;
mod sources;

use tokio;

#[tokio::main]
async fn main() {
    client::connect().await;
}
