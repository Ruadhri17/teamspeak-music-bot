//! Translates server events into `TeamspeakEvent` for the rest of the bot and executes
//! `TeamspeakRequest`s on the connection.
//!
//! - `connect`: building the connection from the config
//! - `event`: `TeamspeakEvent` and translation from tsclientlib events
//! - `frames`: which frame goes out next, the start buffer and the ends of streams
//! - `metrics`: the audio statistics summed up every `logging.metrics_interval`
//! - `home`: the home channel and the decision for `!bot summon`
//! - `request`: `TeamspeakRequest` and executing it on the connection
//! - `error`: `TeamspeakError` for the whole module

mod connect;
mod error;
mod event;
mod frames;
mod home;
mod metrics;
mod request;
#[cfg(test)]
mod tests;

pub use error::{TeamspeakError, TeamspeakResult};
pub use event::{Author, CurrentChannel, ReplyTo, TeamspeakEvent};
pub use request::TeamspeakRequest;

use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    time::Duration,
};

use futures::{StreamExt, future};
use tokio::{
    sync::{
        mpsc::{self, error::TrySendError},
        watch,
    },
    time::{Instant, MissedTickBehavior},
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use tsclientlib::{
    ChannelId, ClientType, CommandError, Connection, DisconnectOptions, InMessage, Invoker,
    MessageHandle, OutCommandExt, Reason, StreamItem, data,
    events::{Event, PropertyId},
};
use tsproto_packets::packets::{AudioData, CodecType, OutAudio};

use frames::Frames;
use home::ChannelInfo;
use metrics::AudioMetrics;

use crate::{
    audio::{self, FRAME_DURATION, Frame, Output},
    config::{ChannelRef, Config, GroupRef, PermissionsConfig, Volume},
    permissions::{self, Group},
    secrets::{TeamspeakSecrets, identity},
};

/// How late a tick may be before the 20 ms grid starts over instead of making up for it.
const RESYNC_AFTER: Duration = Duration::from_millis(200);

/// What the client sends right now. Set by the player, read every tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioControl {
    /// The lowest stream ID that still plays. Frames with a lower ID belong to stopped
    /// tracks and are dropped instead of sent.
    pub min_stream_id: u64,
    /// While `true`, no frames are sent and they wait in the channel.
    pub paused: bool,
    /// Applied right before encoding, so a change is heard in the next frame.
    pub volume: Volume,
}

impl Default for AudioControl {
    fn default() -> Self {
        Self {
            min_stream_id: 0,
            paused: false,
            volume: Volume::FULL,
        }
    }
}

pub struct Client {
    connection: Connection,
    /// To the rest of the bot.
    events: mpsc::Sender<TeamspeakEvent>,
    /// From the rest of the bot.
    requests: mpsc::Receiver<TeamspeakRequest>,
    /// PCM frames from the audio thread, one encoded and sent per tick.
    frames: Frames,
    /// Streams heard to their end that the bot had no room for yet.
    unreported: VecDeque<u64>,
    metrics: AudioMetrics,
    metrics_interval: Duration,
    control: watch::Receiver<AudioControl>,
    /// Volume and Opus for the frame that goes out.
    output: Output,
    /// A frame went out since the last end-of-speech packet.
    talking: bool,
    /// Where an identity upgraded by tsclientlib is saved.
    identity_path: PathBuf,
    /// `home_channel` from the config, `None` = the server's default channel.
    home_config: Option<ChannelRef>,
    /// Known once connected, forgotten on a reconnect.
    home: Option<ChannelId>,
    /// The bot's channel as last sent to the bot, so only changes go out.
    last_channel: Option<CurrentChannel>,
    description: String,
    /// `events.on_disconnect.message`, the reason users see when the bot leaves.
    disconnect_message: Option<String>,
    permissions: PermissionsConfig,
    /// The warning about group names that cannot work was logged.
    warned_group_names: bool,
    /// Why the server removed the bot, e.g. `kicked by Admin`, for the error when the
    /// connection ends.
    left: Option<String>,
    /// Commands waiting for the server's answer, with a label for the log.
    sent: HashMap<MessageHandle, &'static str>,
    shutdown: CancellationToken,
}

impl Client {
    /// Loads the identity (or creates it on the first start) and starts connecting.
    pub fn new(
        config: &Config,
        secrets: &TeamspeakSecrets,
        events: mpsc::Sender<TeamspeakEvent>,
        requests: mpsc::Receiver<TeamspeakRequest>,
        frames: mpsc::Receiver<Frame>,
        control: watch::Receiver<AudioControl>,
        shutdown: CancellationToken,
    ) -> TeamspeakResult<Self> {
        let identity_path = config.paths.state_dir.join(&config.teamspeak.identity_file);
        let identity = identity::load_or_create(&identity_path)?;
        Ok(Self {
            connection: connect::connect(&config.teamspeak, secrets, identity)?,
            events,
            requests,
            frames: Frames::new(frames, audio::frames_in(config.audio.start_buffer)),
            unreported: VecDeque::new(),
            metrics: AudioMetrics::default(),
            metrics_interval: config.logging.metrics_interval,
            control,
            output: Output::new(config.audio.bitrate, config.audio.complexity)
                .map_err(TeamspeakError::Encoder)?,
            talking: false,
            identity_path,
            home_config: config.teamspeak.home_channel.clone(),
            home: None,
            last_channel: None,
            description: config.teamspeak.description.clone(),
            disconnect_message: config.events.on_disconnect.message.clone(),
            permissions: config.permissions.clone(),
            warned_group_names: false,
            sent: HashMap::new(),
            left: None,
            shutdown,
        })
    }

    /// Runs until `shutdown` is cancelled or the connection ends.
    /// tsclientlib sends outgoing commands only while `events()` is polled,
    /// so no branch may block for long.
    pub async fn run(mut self) -> TeamspeakResult<()> {
        let mut ticker = tokio::time::interval(FRAME_DURATION);
        // Keeps the 20 ms grid: a late tick is made up by sending the missed frames at once.
        // `Delay` would start a new grid from each late tick, so every hiccup would send
        // a bit less audio than real time and the listeners' buffers would slowly run dry.
        ticker.set_missed_tick_behavior(MissedTickBehavior::Burst);
        let mut metrics = tokio::time::interval_at(
            Instant::now() + self.metrics_interval,
            self.metrics_interval,
        );

        loop {
            tokio::select! {
                // The `async` block keeps the temporary stream alive while `next()` waits.
                item = async { self.connection.events().next().await } => match item {
                    Some(Ok(item)) => self.handle_item(item),
                    Some(Err(source)) => return Err(TeamspeakError::Connection(source.into())),
                    // Only `shutdown` ends the bot normally, an error lets systemd restart it.
                    None => return Err(TeamspeakError::Closed(self.left.take().unwrap_or_else(
                        || "no reason given".to_owned(),
                    ))),
                },
                // Only logged, a broken connection is reported by the `events()` branch.
                Some(request) = self.requests.recv() => {
                    match request::execute(&mut self.connection, self.home, request) {
                        Ok(sent) => self.track(sent),
                        Err(err) => warn!("{:#}", anyhow::Error::from(err)),
                    }
                }
                scheduled = ticker.tick() => {
                    let late = Instant::now().saturating_duration_since(scheduled);
                    self.metrics.tick(late);
                    if late > RESYNC_AFTER {
                        // After a long stall (a suspended host, an overloaded VPS) making up
                        // for it would send seconds of audio at once, so the grid starts over.
                        warn!("the audio clock was {late:?} late, starting it over");
                        self.metrics.resync();
                        ticker.reset();
                    }
                    self.send_frame();
                }
                _ = metrics.tick() => {
                    match self.metrics.report() {
                        Some(report) if report.trouble => warn!("{}", report.line),
                        Some(report) => debug!("{}", report.line),
                        None => {}
                    }
                }
                _ = self.shutdown.cancelled() => break,
            }
        }

        self.disconnect().await
    }

    fn handle_item(&mut self, item: StreamItem) {
        match item {
            StreamItem::BookEvents(events) => {
                self.forward_events(events);
                self.report_channel();
            }
            // The channel tree is complete only after this, earlier book events have a part of it.
            StreamItem::MessageEvent(InMessage::ChannelListFinished(_)) => {
                self.on_connected();
                self.report_channel();
                self.report_connected();
            }
            StreamItem::IdentityLevelIncreased => self.save_identity(),
            StreamItem::MessageResult(handle, result) => self.command_done(handle, result),
            // tsclientlib reconnects on its own, the setup runs again after that.
            StreamItem::DisconnectedTemporarily(reason) => {
                warn!("disconnected from the server ({reason:?}), reconnecting");
                self.home = None;
                // After the reconnect the channel is sent again, even if it is the same.
                self.last_channel = None;
                // Commands of the old connection get no answer.
                self.sent.clear();
            }
            _ => {}
        }
    }

    fn track(&mut self, sent: Option<request::Sent>) {
        if let Some((handle, label)) = sent {
            self.sent.insert(handle, label);
        }
    }

    /// A refusal usually means the server permissions of the bot need fixing.
    fn command_done(&mut self, handle: MessageHandle, result: Result<(), CommandError>) {
        let label = self.sent.remove(&handle).unwrap_or("a command");
        if let Err(err) = result {
            match err.missing_permission {
                Some(permission) => {
                    warn!("the server refused {label}: {err}, missing permission {permission:?}")
                }
                None => warn!("the server refused {label}: {err}"),
            }
        }
    }

    /// Runs once the channel list is complete: finds the home channel, subscribes to all
    /// channels and sets the description.
    fn on_connected(&mut self) {
        let Ok(state) = self.connection.get_state() else {
            return;
        };
        let channels = channel_infos(state);
        let current = state.clients.get(&state.own_client).map(|bot| bot.channel);

        let home = match home::resolve(self.home_config.as_ref(), &channels) {
            Some(id) => ChannelId(id),
            None => {
                warn!(
                    "teamspeak.home_channel not found on the server, the current channel is home"
                );
                match current {
                    Some(channel) => channel,
                    None => return,
                }
            }
        };
        let name = state
            .channels
            .get(&home)
            .map_or("", |channel| channel.name.as_str());
        info!("home channel: {name} ({})", home.0);
        if current.is_some_and(|channel| channel != home) {
            // The server puts the bot in the default channel when it cannot join the configured one.
            warn!(
                "the bot is not in teamspeak.home_channel, check its password and join permissions"
            );
        }
        self.home = Some(home);

        // Shows whether the server group meant for the bot was kept after a restart.
        if let Some(bot) = state.clients.get(&state.own_client) {
            let groups: Vec<String> = bot
                .server_groups
                .iter()
                .map(|id| match state.server_groups.get(id) {
                    Some(group) if group.is_permanent => format!("{} ({})", group.name, id.0),
                    Some(group) => format!("{} ({}, not saved)", group.name, id.0),
                    None => format!("({})", id.0),
                })
                .collect();
            info!(
                "bot database ID {}, server groups: {}",
                bot.database_id.0,
                groups.join(", ")
            );
        }

        // Clients in other channels are only visible in subscribed channels, `!bot summon`
        // needs the sender's channel. tsclientlib subscribes new channels on its own after this.
        let subscribe = state.server.set_subscribed(true);
        match subscribe.send_with_result(&mut self.connection) {
            Ok(handle) => self.track(Some((handle, "subscribing to all channels"))),
            Err(err) => warn!("could not subscribe to all channels: {err}"),
        }

        // The same request as an event's `description`, so both behave alike.
        // After a reconnect this sets the description from the config again.
        let description = TeamspeakRequest::SetDescription(self.description.clone());
        match request::execute(&mut self.connection, self.home, description) {
            Ok(sent) => self.track(sent),
            Err(err) => warn!("{:#}", anyhow::Error::from(err)),
        }
    }

    /// Never waits for the bot: while a branch of `run` waits, no audio frame goes out,
    /// so a stuck bot would also stop the sound. A full channel drops the event instead.
    fn forward_events(&mut self, events: Vec<tsclientlib::events::Event>) {
        let Ok(state) = self.connection.get_state() else {
            // Happens when the server shuts down: tsclientlib drops the old state and then
            // hands over the last events of that connection.
            debug!("received events without a connection state, skipped");
            return;
        };
        let bot_id = state.own_client;

        for event in &events {
            if let Event::PropertyRemoved {
                id: PropertyId::Client(id),
                invoker,
                extra,
                ..
            } = event
                && *id == bot_id
            {
                self.left = Some(leave_reason(extra.reason, invoker.as_ref()));
            }
        }

        let translated: Vec<TeamspeakEvent> = events
            .iter()
            .filter_map(|event| {
                event::translate(event, bot_id, |invoker| {
                    author(state, &self.permissions, invoker)
                })
            })
            .collect();
        if !translated.is_empty() && !self.warned_group_names {
            self.warned_group_names = true;
            warn_unseen_group_names(state, &self.permissions);
        }
        for event in translated {
            match self.events.try_send(event) {
                Ok(()) => {}
                // Without the event, a chat message may carry anything users wrote.
                Err(TrySendError::Full(_)) => {
                    warn!("the bot is not keeping up, an event from the server was dropped")
                }
                // The receiver is gone, which only happens during shutdown.
                Err(TrySendError::Closed(_)) => {}
            }
        }
    }

    /// Sends the bot's channel when it changed: the bot moved, or someone came or left.
    /// Waits for the home channel, `at_home` needs it. Never waits for the bot, like
    /// `forward_events`.
    fn report_channel(&mut self) {
        let Some(home) = self.home else {
            return;
        };
        let Ok(state) = self.connection.get_state() else {
            return;
        };
        let Some(current) = current_channel(state, home) else {
            return;
        };
        if self.last_channel == Some(current) {
            return;
        }
        match self.events.try_send(TeamspeakEvent::Channel(current)) {
            Ok(()) => self.last_channel = Some(current),
            // Not remembered as sent, so the next change on the server sends it again.
            Err(TrySendError::Full(_)) => {
                warn!("the bot is not keeping up, a change in its channel was not reported")
            }
            // The receiver is gone, which only happens during shutdown.
            Err(TrySendError::Closed(_)) => {}
        }
    }

    /// Tells the bot that the connection is ready, for `on_connect`. Only once the home
    /// channel is known, like `report_channel`.
    fn report_connected(&mut self) {
        if self.home.is_none() {
            return;
        }
        match self.events.try_send(TeamspeakEvent::Connected) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                warn!("the bot is not keeping up, on_connect was not reported")
            }
            // The receiver is gone, which only happens during shutdown.
            Err(TrySendError::Closed(_)) => {}
        }
    }

    /// Saves the identity after tsclientlib raised its level for the server.
    /// Only logged on failure, the connection works anyway, the next start computes it again.
    fn save_identity(&self) {
        if let Some(identity) = self.connection.get_options().get_identity()
            && let Err(err) = identity::save(&self.identity_path, identity)
        {
            warn!("{:#}", anyhow::Error::from(err));
        }
    }

    /// Sends one frame, called every `FRAME_DURATION`.
    fn send_frame(&mut self) {
        let control = *self.control.borrow();
        // While tsclientlib reconnects, frames wait like during a pause, `send_audio` would
        // fail for every one of them.
        if control.paused || self.home.is_none() {
            self.frames.hold(control.min_stream_id);
            self.end_speech();
        } else {
            match self.frames.next(control.min_stream_id, self.talking) {
                Some(mut pcm) => {
                    let started = std::time::Instant::now();
                    let encoded = self.output.encode(&mut pcm, control.volume);
                    self.metrics.sent(started.elapsed());
                    match encoded {
                        Ok(data) => {
                            self.send_audio(&data);
                            self.talking = true;
                        }
                        // One lost frame is a short gap, listeners conceal it.
                        Err(err) => warn!("Opus could not encode a frame: {err}"),
                    }
                }
                None => {
                    if self.talking && self.frames.mid_stream(control.min_stream_id) {
                        self.metrics.underrun();
                    }
                    self.end_speech();
                }
            }
        }
        self.report_played();
    }

    /// Tells the bot which streams were heard to their end. One the bot has no room for
    /// waits for a later tick: a lost end would leave an event waiting for its sound.
    fn report_played(&mut self) {
        self.unreported.extend(self.frames.take_ended());
        while let Some(&stream) = self.unreported.front() {
            match self.events.try_send(TeamspeakEvent::Played(stream)) {
                // `Closed` means the bot is gone, which only happens during shutdown.
                Ok(()) | Err(TrySendError::Closed(_)) => {
                    self.unreported.pop_front();
                }
                Err(TrySendError::Full(_)) => {
                    debug!("the bot is not keeping up, the end of stream {stream} waits");
                    return;
                }
            }
        }
    }

    /// Sends the end-of-speech packet once after the last frame.
    /// Without it, the listeners' decoders conceal the missing packets and produce artifacts.
    fn end_speech(&mut self) {
        if self.talking {
            // An empty packet ends the voice stream (tsclientlib docs of `send_audio`).
            self.send_audio(&[]);
            self.talking = false;
            if let Err(err) = self.output.reset() {
                warn!("Opus encoder could not be reset: {err}");
            }
        }
    }

    fn send_audio(&mut self, data: &[u8]) {
        let packet = OutAudio::new(&AudioData::C2S {
            id: 0,
            codec: CodecType::OpusMusic,
            data,
        });
        if let Err(err) = self.connection.send_audio(packet) {
            warn!("audio could not be sent: {err}");
        }
    }

    /// Disconnects cleanly, so the server does not wait for a timeout.
    async fn disconnect(mut self) -> TeamspeakResult<()> {
        let mut options = DisconnectOptions::new();
        if let Some(message) = &self.disconnect_message {
            // tsclientlib sends the message only together with a reason.
            options = options
                .reason(Reason::Clientdisconnect)
                .message(message.as_str());
        }
        self.connection
            .disconnect(options)
            .map_err(|source| TeamspeakError::Request(source.into()))?;
        // Polls until the server confirms, the disconnect is sent only while polling.
        self.connection
            .events()
            .for_each(|_| future::ready(()))
            .await;
        Ok(())
    }
}

/// The role and channel of a message's author, from what the bot sees right now.
/// An author the bot cannot see gets no groups and counts as outside its channel.
fn author(state: &data::Connection, config: &PermissionsConfig, invoker: &Invoker) -> Author {
    let client = state.clients.get(&invoker.id);
    let bot_channel = state.clients.get(&state.own_client).map(|bot| bot.channel);
    let groups: Vec<Group> = client
        .map(|client| {
            client
                .server_groups
                .iter()
                .map(|id| Group {
                    id: id.0,
                    name: state.server_groups.get(id).map(|group| group.name.as_str()),
                })
                .collect()
        })
        .unwrap_or_default();
    Author {
        id: invoker.id.0,
        name: invoker.name.clone(),
        role: permissions::role(config, &groups),
        in_bot_channel: client.is_some_and(|client| Some(client.channel) == bot_channel),
    }
}

/// Group names in `[permissions]` match only groups the bot can see. Checked at the first
/// message, the server sends its group list after the channel list.
fn warn_unseen_group_names(state: &data::Connection, config: &PermissionsConfig) {
    let unseen: Vec<&str> = config
        .admin_groups
        .iter()
        .chain(&config.blocked_groups)
        .filter_map(|group| match group {
            GroupRef::Name(name) => Some(name.as_str()),
            GroupRef::Id(_) => None,
        })
        .filter(|name| {
            !state
                .server_groups
                .values()
                .any(|group| permissions::same_name(&group.name, name))
        })
        .collect();
    if unseen.is_empty() {
        return;
    }
    if state.server_groups.is_empty() {
        warn!(
            "the bot cannot see the server groups, only group IDs in [permissions] work \
             (needs b_virtualserver_servergroup_list)"
        );
    } else {
        warn!(
            "[permissions] names server groups that do not exist: {}",
            unseen.join(", ")
        );
    }
}

/// Where the bot is and how many people are with it. Server query clients are not people.
/// `None` while the bot itself is not in the state yet.
fn current_channel(state: &data::Connection, home: ChannelId) -> Option<CurrentChannel> {
    let channel = state.clients.get(&state.own_client)?.channel;
    let others = state
        .clients
        .values()
        .filter(|client| {
            client.id != state.own_client
                && client.channel == channel
                && matches!(client.client_type, ClientType::Normal)
        })
        .count();
    Some(CurrentChannel {
        id: channel.0,
        at_home: channel == home,
        others,
    })
}

/// Every channel on the server, for finding one from the config by ID or path.
/// Borrows the names from `state`, so the list lives only as long as that borrow.
fn channel_infos(state: &data::Connection) -> Vec<ChannelInfo<'_>> {
    state
        .channels
        .values()
        .map(|channel| ChannelInfo {
            id: channel.id.0,
            // 0 for a top-level channel.
            parent: channel.parent.0,
            name: &channel.name,
            is_default: channel.is_default.unwrap_or(false),
        })
        .collect()
}

/// `kicked by Admin`, `banned by Admin`, or the reason as tsclientlib names it.
fn leave_reason(reason: Option<Reason>, invoker: Option<&Invoker>) -> String {
    let action = match reason {
        Some(Reason::KickServer) => "kicked".to_owned(),
        Some(Reason::KickServerBan) => "banned".to_owned(),
        Some(other) => format!("{other:?}"),
        None => "removed".to_owned(),
    };
    match invoker {
        Some(invoker) => format!("{action} by {}", invoker.name),
        None => action,
    }
}
