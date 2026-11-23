use std::{fs, num::NonZeroUsize, os::unix::fs::PermissionsExt};

use tempfile::TempDir;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::{
    audio::{AudioInput, PlayResult},
    client::{AudioControl, ClientEvent, ClientRequest, ReplyTo},
    commands::Command,
    config::{Config, SourceKind},
    player::{
        Launcher, Player,
        state::{Ended, PlaybackId, State},
    },
};

pub struct FakeLauncher;

impl Launcher for FakeLauncher {
    fn launch(
        &mut self,
        _input: AudioInput,
        _stream: u64,
        _stop: CancellationToken,
        _on_done: Box<dyn FnOnce(PlayResult) + Send>,
    ) {
    }
}

pub struct Harness {
    pub player: Player,
    pub dir: TempDir,
    pub scripts: Option<TempDir>,
    control: watch::Receiver<AudioControl>,
    _events: mpsc::Sender<ClientEvent>,
    _requests: mpsc::Receiver<ClientRequest>,
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
        });
        harness.scripts = Some(scripts);
        harness
    }

    pub fn with_config(edit: impl FnOnce(&mut Config)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a.mp3", "b.mp3", "c.mp3"] {
            fs::write(dir.path().join(name), b"audio").unwrap();
        }

        let mut config = Config::default();
        config.sources.default = SourceKind::Local;
        config.sources.local.music_dir = Some(dir.path().to_path_buf());
        config.sources.youtube.yt_dlp_path = dir.path().join("no-yt-dlp");
        edit(&mut config);

        let (events_tx, events_rx) = mpsc::channel(8);
        let (requests_tx, requests_rx) = mpsc::channel(8);
        let (control_tx, control_rx) = watch::channel(AudioControl::default());
        let player = Player::new(
            events_rx,
            requests_tx,
            control_tx,
            Box::new(FakeLauncher),
            &config,
        );

        Self {
            player,
            dir,
            scripts: None,
            control: control_rx,
            _events: events_tx,
            _requests: requests_rx,
        }
    }

    pub async fn send(&mut self, text: &str) -> Option<String> {
        self.send_from(ReplyTo::Channel, text).await
    }

    pub async fn send_from(&mut self, reply_to: ReplyTo, text: &str) -> Option<String> {
        let command = Command::parse(text).unwrap().unwrap();
        self.player.execute(command, reply_to).await
    }

    pub async fn next_lookup(&mut self) {
        let joined = self.player.lookups.join_next_with_id().await.unwrap();
        self.player.lookup_done(joined);
    }

    pub async fn add(&mut self, names: &[&str]) {
        for name in names {
            assert_eq!(self.send(&format!("!song add {name}")).await, None);
        }
        self.settle().await;
    }

    pub async fn add_outbox(&mut self, text: &str) -> Vec<(ReplyTo, String)> {
        assert_eq!(self.send(text).await, None);
        self.settle().await;
        self.outbox()
    }

    pub async fn settle(&mut self) {
        while let Some(joined) = self.player.lookups.join_next_with_id().await {
            self.player.lookup_done(joined);
        }
    }

    pub fn playing(&self) -> Option<&str> {
        match &self.player.state {
            State::Playing(playback) => Some(&playback.track.title),
            _ => None,
        }
    }

    pub fn paused(&self) -> Option<&str> {
        match &self.player.state {
            State::Paused(playback) => Some(&playback.track.title),
            _ => None,
        }
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.player.state, State::Idle)
    }

    pub fn current_id(&self) -> PlaybackId {
        self.player.state.playback().unwrap().id
    }

    pub fn current_stop(&self) -> CancellationToken {
        self.player.state.playback().unwrap().stop.clone()
    }

    pub fn finish_current(&mut self) {
        let id = self.current_id();
        self.player.finished(Ended { id, failure: None });
    }

    pub fn fail_current(&mut self, reason: &str) {
        let id = self.current_id();
        self.player.finished(Ended {
            id,
            failure: Some(reason.to_owned()),
        });
    }

    pub fn outbox(&mut self) -> Vec<(ReplyTo, String)> {
        std::mem::take(&mut self.player.outbox)
    }

    pub fn control(&self) -> AudioControl {
        *self.control.borrow()
    }

    pub fn queued(&self) -> Vec<String> {
        let size = NonZeroUsize::new(1000).unwrap();
        self.player
            .queue
            .page(NonZeroUsize::MIN, size)
            .unwrap()
            .entries
            .into_iter()
            .map(|(_, track)| track.title.clone())
            .collect()
    }
}

pub fn on_channel(text: &str) -> Vec<(ReplyTo, String)> {
    vec![(ReplyTo::Channel, text.to_owned())]
}
