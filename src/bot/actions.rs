//! Running the actions of events: the message, the sound, and after the sound the move,
//! description and nickname. Also the bot's own rules: clearing the queue at home.
//!
//! The rest of an event waits for its sound in `Bot::waiting`. The player reports how the
//! sound ended:
//! - played to its end: the rest runs, after a welcome the waiting tracks start
//! - stopped by music (`!song add`, `!song resume`): the goodbye is over, the bot stays
//! - stopped by a command with nothing to play after it (`!song next`, `!queue clear`):
//!   the sound is skipped, the rest runs, so the bot still leaves
//! - the bot moved, or another event's sound: the rest is dropped

use std::{num::NonZeroUsize, path::Path};

use tokio::{task::JoinError, time::Instant};
use tracing::{debug, info, warn};
use url::Url;

use super::Bot;
use crate::{
    config::{Sound, YoutubeConfig},
    events::{Action, Trigger},
    sources::{Location, QueryType, TrackRef, youtube},
    teamspeak::{ReplyTo, TeamspeakRequest},
};

/// A finished lookup of a sound given as a link.
pub(super) type SoundLookup = (Trigger, Result<TrackRef, String>);

impl Bot {
    /// Files are ready at once, links are looked up in the background, once. An event that
    /// fires before its lookup is done runs without the sound.
    pub(super) fn prepare_sounds(&mut self) {
        for trigger in Trigger::ALL {
            match self.events.event(trigger).sound.clone() {
                None => {}
                Some(Sound::File(path)) => {
                    let track = TrackRef {
                        title: file_title(&path),
                        location: Location::File(path),
                    };
                    self.sounds.insert(trigger, track);
                }
                Some(Sound::Link(url)) => {
                    let youtube = self.sources.youtube.clone();
                    self.sound_lookups
                        .spawn(async move { (trigger, find_sound(youtube, url).await) });
                }
            }
        }
    }

    pub(super) fn sound_found(&mut self, joined: Result<SoundLookup, JoinError>) {
        match joined {
            Ok((trigger, Ok(track))) => {
                info!("{} sound: {}", trigger.name(), track.title);
                self.sounds.insert(trigger, track);
            }
            Ok((trigger, Err(err))) => warn!(
                "the {} sound could not be found, the event runs without it: {err}",
                trigger.name()
            ),
            Err(err) => warn!("the lookup of an event sound panicked: {err}"),
        }
    }

    pub(super) fn run_actions(&mut self, actions: Vec<Action>) {
        for action in actions {
            debug!("event action: {action:?}");
            match action {
                Action::Run(trigger) => self.run_event(trigger),
                // Nobody listens at home, the music stops.
                Action::StopAtHome => self.queue_clear(),
                Action::Moved { home } => {
                    // The actions of an event belong to the channel it fired in.
                    self.waiting = None;
                    self.player.stop_sound();
                    self.report();
                    // Home always means a fresh start, whoever moved the bot.
                    if home {
                        self.queue_clear();
                    }
                }
            }
        }
    }

    /// The message at once, then the sound, the rest after the sound.
    fn run_event(&mut self, trigger: Trigger) {
        // The text comes from the admin, it is sent as written, BBCode included.
        if let Some(text) = self.events.event(trigger).message.clone() {
            self.say(ReplyTo::Channel, text);
        }
        if self.events.event(trigger).sound.is_some() && self.play_event_sound(trigger) {
            self.waiting = Some(trigger);
        } else {
            self.run_rest(trigger);
        }
    }

    /// `true` when the sound started, its end runs the rest.
    fn play_event_sound(&mut self, trigger: Trigger) -> bool {
        let Some(sound) = self.sounds.get(&trigger).cloned() else {
            warn!(
                "the {} sound is not ready (its link is still looked up or failed), skipped",
                trigger.name()
            );
            return false;
        };
        // A welcome never ends a paused track, the pause is kept.
        if trigger == Trigger::Party && !self.player.is_idle() {
            debug!("{} sound skipped, a track is paused", trigger.name());
            return false;
        }
        // An earlier event's sound is stopped by this one, its rest is dropped.
        self.waiting = None;
        match self.player.play_sound(sound) {
            Ok(()) => {
                self.report();
                true
            }
            Err(err) => {
                warn!("the {} sound could not be played: {err}", trigger.name());
                false
            }
        }
    }

    /// The actions after the sound: move, description, nickname.
    pub(super) fn run_rest(&mut self, trigger: Trigger) {
        let event = self.events.event(trigger);
        let requests = [
            event.move_to.clone().map(TeamspeakRequest::Move),
            event
                .description
                .clone()
                .map(TeamspeakRequest::SetDescription),
            event.nickname.clone().map(TeamspeakRequest::SetNickname),
        ];
        self.outbox.extend(requests.into_iter().flatten());
        self.events.actions_done(trigger, Instant::now());
    }

    /// The sound of an event ended. The rest of the event runs, and after a welcome the
    /// tracks that waited for it start.
    pub(super) fn sound_ended(&mut self) {
        let Some(trigger) = self.waiting.take() else {
            return;
        };
        if trigger == Trigger::Party {
            self.player.start_queue();
        }
        self.run_rest(trigger);
    }

    /// The sound was stopped before its end. Music that started ends a goodbye, otherwise
    /// the sound is only skipped and the rest runs.
    pub(super) fn sound_interrupted(&mut self) {
        let Some(trigger) = self.waiting.take() else {
            return;
        };
        if trigger.is_leaving() && self.player.is_playing() {
            debug!("{} interrupted by music, the bot stays", trigger.name());
            return;
        }
        self.run_rest(trigger);
    }

    /// Music wanted during a goodbye (`!song add`, `!song resume`) ends it: the bot stays,
    /// the rest is dropped and the queue starts. Without tracks to play the goodbye goes
    /// on, or the bot would stay away from home with nothing to do.
    /// A welcome is not ended, the music waits for it.
    pub(super) fn end_goodbye(&mut self) {
        if self.waiting.is_some_and(Trigger::is_leaving) && !self.player.queue_is_empty() {
            debug!("music during a goodbye, the bot stays");
            self.player.stop_sound();
            self.player.start_queue();
            // After the queue started, so `sound_interrupted` sees the music and drops the rest.
            self.report();
        }
    }
}

/// One video from a link. A playlist gives its first video.
async fn find_sound(youtube: YoutubeConfig, url: Url) -> Result<TrackRef, String> {
    let found = youtube::resolve(&youtube, &QueryType::Link(url), NonZeroUsize::MIN)
        .await
        .map_err(|err| err.to_string())?;
    if found.playlist.is_some() {
        warn!("an event sound is a playlist, only its first video plays");
    }
    found
        .tracks
        .into_iter()
        .next()
        .ok_or_else(|| "the link has no video".to_string())
}

/// The file name for the log, the whole path when it has none.
fn file_title(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}
