use std::{fs, num::NonZeroUsize, os::unix::fs::PermissionsExt, path::Path};

use tempfile::TempDir;
use tokio::sync::{mpsc, watch};

use crate::{
    audio::{AudioInput, PlayResult},
    bot::Bot,
    config::{Config, SourceKind},
    events::Action,
    permissions::Role,
    player::{Ended, Launcher, PlaybackId},
    process::Stopper,
    teamspeak::{AudioControl, Author, CurrentChannel, ReplyTo, TeamspeakEvent, TeamspeakRequest},
};

pub struct FakeLauncher;

impl Launcher for FakeLauncher {
    fn launch(
        &mut self,
        _input: AudioInput,
        _stream: u64,
        _stop: Stopper,
        _on_done: Box<dyn FnOnce(PlayResult) + Send>,
    ) {
    }
}

pub struct Harness {
    pub bot: Bot,
    pub dir: TempDir,
    pub scripts: Option<TempDir>,
    control: watch::Receiver<AudioControl>,
    _teamspeak_events: mpsc::Sender<TeamspeakEvent>,
    _teamspeak_requests: mpsc::Receiver<TeamspeakRequest>,
}

impl Harness {
    pub fn new() -> Self {
        Self::with_page_size(15)
    }

    pub fn with_page_size(page_size: usize) -> Self {
        Self::with_config(|config| {
            config.queue.show_page_size = NonZeroUsize::new(page_size).unwrap();
        })
    }

    pub fn with_yt_dlp(script: &str) -> Self {
        Self::with_yt_dlp_and(script, |_| {})
    }

    pub fn with_yt_dlp_and(script: &str, edit: impl FnOnce(&mut Config)) -> Self {
        let scripts = tempfile::tempdir().unwrap();
        let path = scripts.path().join("yt-dlp");
        let source = scripts.path().join("yt-dlp.src");
        fs::write(&source, format!("#!/bin/sh\n{script}\n")).unwrap();
        let copied = std::process::Command::new("cp")
            .arg(&source)
            .arg(&path)
            .status()
            .unwrap();
        assert!(copied.success());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();

        let mut harness = Self::with_config(|config| {
            config.sources.default = SourceKind::Youtube;
            config.sources.youtube.yt_dlp_path = path;
            edit(config);
        });
        harness.scripts = Some(scripts);
        harness
    }

    pub fn with_config(edit: impl FnOnce(&mut Config)) -> Self {
        Self::with_config_in(|config, _| edit(config))
    }

    pub fn with_config_in(edit: impl FnOnce(&mut Config, &Path)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a.mp3", "b.mp3", "c.mp3", "bye.wav", "hi.wav"] {
            fs::write(dir.path().join(name), b"audio").unwrap();
        }

        let mut config = Config::default();
        config.sources.default = SourceKind::Local;
        config.sources.local.music_dir = Some(dir.path().to_path_buf());
        config.sources.youtube.yt_dlp_path = dir.path().join("no-yt-dlp");
        edit(&mut config, dir.path());

        let (events_tx, events_rx) = mpsc::channel(8);
        let (requests_tx, requests_rx) = mpsc::channel(8);
        let (control_tx, control_rx) = watch::channel(AudioControl::default());
        let bot = Bot::new(
            events_rx,
            requests_tx,
            control_tx,
            Box::new(FakeLauncher),
            &config,
        );

        Self {
            bot,
            dir,
            scripts: None,
            control: control_rx,
            _teamspeak_events: events_tx,
            _teamspeak_requests: requests_rx,
        }
    }

    pub fn run(&mut self, actions: &[Action]) {
        self.bot.run_actions(actions.to_vec());
    }

    pub fn channel(&mut self, current: CurrentChannel) {
        self.bot.handle_event(TeamspeakEvent::Channel(current));
    }

    pub fn connected(&mut self) {
        self.bot.handle_event(TeamspeakEvent::Connected);
    }

    pub fn sound(&self) -> Option<&str> {
        self.bot.player.sound()
    }

    pub fn send(&mut self, text: &str) -> Option<String> {
        self.send_from(ReplyTo::Channel, text)
    }

    pub fn send_from(&mut self, reply_to: ReplyTo, text: &str) -> Option<String> {
        let id = match reply_to {
            ReplyTo::Client(id) => id,
            ReplyTo::Channel => 1,
        };
        self.send_as(reply_to, &user(id), text)
    }

    pub fn send_as(&mut self, reply_to: ReplyTo, author: &Author, text: &str) -> Option<String> {
        self.bot.command(text, reply_to, author)
    }

    pub async fn next_lookup(&mut self) {
        let joined = self.bot.lookups.join_next_with_id().await.unwrap();
        self.bot.lookup_done(joined);
    }

    pub async fn add(&mut self, names: &[&str]) {
        for name in names {
            assert_eq!(self.send(&format!("!song add {name}")), None);
        }
        self.settle().await;
    }

    pub async fn add_outbox(&mut self, text: &str) -> Vec<(ReplyTo, String)> {
        assert_eq!(self.send(text), None);
        self.settle().await;
        self.outbox()
    }

    pub async fn settle(&mut self) {
        while let Some(joined) = self.bot.lookups.join_next_with_id().await {
            self.bot.lookup_done(joined);
        }
    }

    pub fn playing(&self) -> Option<&str> {
        self.bot.player.playing()
    }

    pub fn paused(&self) -> Option<&str> {
        self.bot.player.paused()
    }

    pub fn is_idle(&self) -> bool {
        self.bot.player.is_idle()
    }

    pub fn current_id(&self) -> PlaybackId {
        self.bot.player.current_id()
    }

    pub fn current_stop(&self) -> Stopper {
        self.bot.player.current_stop()
    }

    pub fn finish_current(&mut self) {
        let id = self.current_id();
        self.bot.finished(Ended {
            id,
            failure: None,
            end_frame: true,
        });
    }

    pub fn finish_current_id(&mut self, id: PlaybackId) {
        self.bot.finished(Ended {
            id,
            failure: None,
            end_frame: true,
        });
    }

    pub fn track_heard(&mut self) {
        let id = self.current_id();
        self.finish_current();
        self.played(id);
    }

    pub fn sound_heard(&mut self) {
        let id = self.current_id();
        self.bot.finished(Ended {
            id,
            failure: None,
            end_frame: true,
        });
        self.played(id);
    }

    pub fn played(&mut self, id: PlaybackId) {
        self.bot.handle_event(TeamspeakEvent::Played(id.0));
    }

    pub fn fail_current(&mut self, reason: &str) {
        let id = self.current_id();
        self.bot.finished(Ended {
            id,
            failure: Some(reason.to_owned()),
            end_frame: true,
        });
    }

    pub fn outbox(&mut self) -> Vec<(ReplyTo, String)> {
        self.teamspeak_requests()
            .into_iter()
            .map(|request| match request {
                TeamspeakRequest::SendMessage { to, text } => (to, text),
                other => panic!("not a message: {other:?}"),
            })
            .collect()
    }

    pub fn teamspeak_requests(&mut self) -> Vec<TeamspeakRequest> {
        std::mem::take(&mut self.bot.outbox)
    }

    pub fn control(&self) -> AudioControl {
        *self.control.borrow()
    }

    pub fn queued(&self) -> Vec<String> {
        self.bot.player.queued()
    }
}

pub fn reply(text: &str) -> Option<String> {
    Some(text.to_string())
}

pub fn on_channel(text: &str) -> Vec<(ReplyTo, String)> {
    vec![(ReplyTo::Channel, text.to_owned())]
}

pub fn author(id: u16, role: Role, in_bot_channel: bool) -> Author {
    Author {
        id,
        name: format!("user{id}"),
        role,
        in_bot_channel,
    }
}

pub fn user(id: u16) -> Author {
    author(id, Role::User, true)
}
