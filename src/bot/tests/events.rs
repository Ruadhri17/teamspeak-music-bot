use std::{fs, time::Duration};

use super::harness::{Harness, on_channel};
use crate::{
    config::{Destination, Event, Sound},
    events::{Action, Trigger},
    teamspeak::{CurrentChannel, ReplyTo, TeamspeakRequest},
    template::Template,
};

fn text(text: &str) -> Option<Template> {
    Some(text.parse().unwrap())
}

const GO_HOME: TeamspeakRequest = TeamspeakRequest::Move(Destination::Home);

fn say(text: &str) -> TeamspeakRequest {
    TeamspeakRequest::SendMessage {
        to: ReplyTo::Channel,
        text: text.to_owned(),
    }
}

fn describe(text: &str) -> TeamspeakRequest {
    TeamspeakRequest::SetDescription(text.to_owned())
}

fn with_sounds() -> Harness {
    Harness::with_config_in(|config, dir| {
        let goodbye = Event {
            delay: Duration::from_secs(60),
            message: text("bye"),
            sound: Some(Sound::File(dir.join("bye.wav"))),
            move_to: Some(Destination::Home),
            ..Event::default()
        };
        config.events.on_alone = goodbye.clone();
        config.events.on_idle = goodbye;
        config.events.on_party = Event {
            sound: Some(Sound::File(dir.join("hi.wav"))),
            description: text("hello"),
            ..Event::default()
        };
    })
}

fn alone() -> Action {
    Action::Run(Trigger::Alone)
}

fn party() -> Action {
    Action::Run(Trigger::Party)
}

#[tokio::test]
async fn goodbye_plays_its_sound_then_goes_home() {
    let mut h = with_sounds();
    h.run(&[alone()]);
    assert_eq!(h.teamspeak_requests(), [say("bye")]);
    assert_eq!(h.sound(), Some("bye.wav"));
    h.sound_heard();
    assert_eq!(h.teamspeak_requests(), [GO_HOME]);
    assert!(h.is_idle());
}

#[tokio::test]
async fn a_decoded_sound_waits_until_it_is_heard() {
    let mut h = with_sounds();
    h.run(&[alone()]);
    h.teamspeak_requests();
    let id = h.current_id();
    h.finish_current();
    assert!(h.teamspeak_requests().is_empty());
    assert_eq!(h.sound(), Some("bye.wav"));
    h.played(id);
    assert_eq!(h.teamspeak_requests(), [GO_HOME]);
}

#[tokio::test]
async fn the_played_end_of_a_stopped_sound_changes_nothing() {
    let mut h = with_sounds();
    h.run(&[alone()]);
    h.teamspeak_requests();
    let old = h.current_id();
    h.run(&[Action::Moved { home: false }]);
    h.played(old);
    assert!(h.teamspeak_requests().is_empty());
}

#[tokio::test]
async fn the_played_end_of_a_track_is_not_the_end_of_a_sound() {
    let mut h = with_sounds();
    h.add(&["a.mp3", "b.mp3"]).await;
    let first = h.current_id();
    h.finish_current();
    h.played(first);
    assert_eq!(h.playing(), Some("b"));
    assert!(h.teamspeak_requests().is_empty());
}

#[tokio::test]
async fn without_a_sound_the_rest_runs_at_once() {
    let mut h = Harness::with_config(|config| {
        config.events.on_alone = Event {
            delay: Duration::from_secs(60),
            move_to: Some(Destination::Home),
            nickname: text("Resting Bot"),
            ..Event::default()
        };
    });
    h.run(&[alone()]);
    assert_eq!(
        h.teamspeak_requests(),
        [GO_HOME, TeamspeakRequest::SetNickname("Resting Bot".into())]
    );
}

#[tokio::test]
async fn an_event_without_actions_does_nothing() {
    let mut h = Harness::with_config(|config| {
        config.events.on_alone = Event {
            delay: Duration::from_secs(60),
            ..Event::default()
        };
    });
    h.run(&[alone()]);
    assert!(h.teamspeak_requests().is_empty());
    assert!(h.is_idle());
}

#[tokio::test]
async fn a_sound_is_not_music() {
    let mut h = with_sounds();
    h.run(&[alone()]);
    assert!(!h.bot.player.is_playing());
    assert_eq!(h.send("!song current"), Some("nothing is playing".into()));
    assert_eq!(h.send("!queue show"), Some("the queue is empty".into()));
    assert_eq!(h.send("!song pause"), None);
    assert_eq!(h.sound(), Some("bye.wav"));
}

#[tokio::test]
async fn a_new_song_ends_the_goodbye_and_the_bot_stays() {
    let mut h = with_sounds();
    h.run(&[alone()]);
    h.teamspeak_requests();
    h.add(&["a.mp3"]).await;
    assert_eq!(h.playing(), Some("a"));
    assert!(h.teamspeak_requests().is_empty());
    h.finish_current();
    assert!(h.teamspeak_requests().is_empty());
}

#[tokio::test]
async fn resume_ends_the_goodbye_when_songs_wait() {
    let mut h = with_sounds();
    h.add(&["a.mp3", "b.mp3"]).await;
    h.run(&[alone()]);
    assert_eq!(h.sound(), Some("bye.wav"));
    h.teamspeak_requests();
    assert_eq!(h.send("!song resume"), None);
    assert_eq!(h.playing(), Some("b"));
    assert!(h.teamspeak_requests().is_empty());
}

#[tokio::test]
async fn resume_with_an_empty_queue_keeps_the_goodbye() {
    let mut h = with_sounds();
    h.run(&[alone()]);
    h.teamspeak_requests();
    h.send("!song resume");
    assert_eq!(h.sound(), Some("bye.wav"));
    h.sound_heard();
    assert_eq!(h.teamspeak_requests(), [GO_HOME]);
}

#[tokio::test]
async fn next_skips_the_goodbye_sound_and_the_bot_leaves() {
    let mut h = with_sounds();
    h.run(&[alone()]);
    h.teamspeak_requests();
    assert_eq!(h.send("!song next"), None);
    assert_eq!(h.teamspeak_requests(), [GO_HOME]);
    assert!(h.is_idle());
}

#[tokio::test]
async fn queue_clear_skips_the_goodbye_sound_and_the_bot_leaves() {
    let mut h = with_sounds();
    h.run(&[alone()]);
    h.teamspeak_requests();
    assert_eq!(h.send("!queue clear"), None);
    assert_eq!(h.teamspeak_requests(), [GO_HOME]);
}

#[tokio::test]
async fn moving_the_bot_drops_the_rest() {
    let mut h = with_sounds();
    h.run(&[alone()]);
    h.teamspeak_requests();
    h.run(&[Action::Moved { home: false }]);
    assert!(h.is_idle());
    assert!(h.teamspeak_requests().is_empty());
}

#[tokio::test]
async fn a_late_end_of_a_stopped_sound_changes_nothing() {
    let mut h = with_sounds();
    h.run(&[alone()]);
    h.teamspeak_requests();
    let old = h.current_id();
    h.run(&[Action::Moved { home: false }]);
    h.bot.finished(crate::player::Ended {
        id: old,
        failure: None,
    });
    assert!(h.teamspeak_requests().is_empty());
}

#[tokio::test]
async fn arriving_home_clears_the_queue() {
    let mut h = with_sounds();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]).await;
    h.run(&[Action::Moved { home: true }]);
    assert!(h.is_idle());
    assert!(h.queued().is_empty());
}

#[tokio::test]
async fn alone_at_home_stops_the_music() {
    let mut h = with_sounds();
    h.add(&["a.mp3", "b.mp3"]).await;
    h.run(&[Action::StopAtHome]);
    assert!(h.is_idle());
    assert!(h.queued().is_empty());
    assert!(h.teamspeak_requests().is_empty());
}

#[tokio::test]
async fn songs_wait_for_the_welcome_and_start_after_it() {
    let mut h = with_sounds();
    h.run(&[party()]);
    assert_eq!(h.sound(), Some("hi.wav"));
    h.add(&["a.mp3"]).await;
    assert_eq!(h.sound(), Some("hi.wav"));
    assert_eq!(h.queued(), ["a"]);
    h.sound_heard();
    assert_eq!(h.playing(), Some("a"));
    assert_eq!(h.teamspeak_requests(), [describe("hello")]);
}

#[tokio::test]
async fn an_interrupted_welcome_still_runs_the_rest() {
    let mut h = with_sounds();
    h.run(&[party()]);
    h.add(&["a.mp3"]).await;
    assert_eq!(h.send("!song next"), None);
    assert_eq!(h.playing(), Some("a"));
    assert_eq!(h.teamspeak_requests(), [describe("hello")]);
}

#[tokio::test]
async fn a_paused_track_keeps_its_pause_and_skips_the_welcome_sound() {
    let mut h = with_sounds();
    h.add(&["a.mp3"]).await;
    h.send("!song pause");
    h.run(&[party()]);
    assert_eq!(h.paused(), Some("a"));
    assert_eq!(h.sound(), None);
    assert_eq!(h.teamspeak_requests(), [describe("hello")]);
}

#[tokio::test]
async fn a_goodbye_replaces_a_paused_track() {
    let mut h = with_sounds();
    h.add(&["a.mp3"]).await;
    h.send("!song pause");
    h.run(&[Action::Run(Trigger::Idle)]);
    assert_eq!(h.paused(), None);
    assert_eq!(h.sound(), Some("bye.wav"));
    assert!(!h.control().paused);
}

#[tokio::test]
async fn a_second_event_sound_drops_the_rest_of_the_first() {
    let mut h = with_sounds();
    h.run(&[party()]);
    h.run(&[alone()]);
    assert_eq!(h.sound(), Some("bye.wav"));
    h.teamspeak_requests();
    h.sound_heard();
    assert_eq!(h.teamspeak_requests(), [GO_HOME]);
}

#[tokio::test]
async fn a_missing_sound_file_runs_the_rest_at_once() {
    let mut h = with_sounds();
    fs::remove_file(h.dir.path().join("bye.wav")).unwrap();
    h.run(&[alone()]);
    assert_eq!(h.teamspeak_requests(), [say("bye"), GO_HOME]);
    assert!(h.is_idle());
}

#[tokio::test]
async fn a_failed_sound_still_runs_the_rest() {
    let mut h = with_sounds();
    h.run(&[alone()]);
    h.teamspeak_requests();
    let id = h.current_id();
    h.bot.finished(crate::player::Ended {
        id,
        failure: Some("playback failed".into()),
    });
    assert_eq!(h.teamspeak_requests(), [GO_HOME]);
}

#[tokio::test]
async fn entering_a_channel_with_people_welcomes_them() {
    let mut h = with_sounds();
    h.channel(CurrentChannel {
        id: 1,
        at_home: true,
        others: 0,
    });
    h.channel(CurrentChannel {
        id: 2,
        at_home: false,
        others: 2,
    });
    assert_eq!(h.sound(), Some("hi.wav"));
    assert!(h.outbox().is_empty());
}

#[tokio::test]
async fn the_event_message_is_sent_as_written() {
    let mut h = Harness::with_config(|config| {
        config.events.on_party = Event {
            message: text("[b]Hi[/b] everyone"),
            ..Event::default()
        };
    });
    h.run(&[party()]);
    assert_eq!(h.outbox(), on_channel("[b]Hi[/b] everyone"));
}

#[tokio::test]
async fn next_with_songs_waiting_ends_the_goodbye_and_the_bot_stays() {
    let mut h = with_sounds();
    h.add(&["a.mp3", "b.mp3"]).await;
    h.run(&[alone()]);
    h.teamspeak_requests();
    assert_eq!(h.send("!song next"), None);
    assert_eq!(h.playing(), Some("b"));
    assert!(h.teamspeak_requests().is_empty());
}

#[tokio::test]
async fn a_finished_leaving_event_lets_the_bot_welcome_people_again() {
    let mut h = Harness::with_config_in(|config, dir| {
        config.events.on_alone = Event {
            delay: Duration::from_secs(60),
            message: text("anyone there?"),
            ..Event::default()
        };
        config.events.on_party = Event {
            sound: Some(Sound::File(dir.join("hi.wav"))),
            ..Event::default()
        };
    });
    h.channel(CurrentChannel {
        id: 1,
        at_home: true,
        others: 0,
    });
    h.channel(CurrentChannel {
        id: 2,
        at_home: false,
        others: 0,
    });
    let later = tokio::time::Instant::now() + Duration::from_secs(61);
    let actions = h.bot.events.tick(later);
    assert_eq!(actions, [alone()]);
    h.bot.run_actions(actions);
    assert_eq!(h.outbox(), on_channel("anyone there?"));
    h.channel(CurrentChannel {
        id: 2,
        at_home: false,
        others: 1,
    });
    assert_eq!(h.sound(), Some("hi.wav"));
}

fn song_start(event: Event) -> Harness {
    Harness::with_config(|config| {
        config.teamspeak.nickname = "Music Bot".into();
        config.teamspeak.description = "Drag me to your channel".into();
        config.events.on_song_start = event;
    })
}

#[tokio::test]
async fn song_start_fills_in_the_song_and_its_source() {
    let mut h = song_start(Event {
        message: text("Now playing: {song} from {source}"),
        description: text("{default} | {song}"),
        ..Event::default()
    });
    h.add(&["a.mp3"]).await;
    assert_eq!(
        h.teamspeak_requests(),
        [
            say("Now playing: a from local file"),
            describe("Drag me to your channel | a")
        ]
    );
    h.finish_current();
    assert!(h.teamspeak_requests().is_empty());
}

#[tokio::test]
async fn song_start_runs_for_every_song() {
    let mut h = song_start(Event {
        message: text("Now playing: {song}"),
        ..Event::default()
    });
    h.add(&["a.mp3", "b.mp3"]).await;
    assert_eq!(h.outbox(), on_channel("Now playing: a"));
    assert_eq!(h.send("!song next"), None);
    assert_eq!(h.outbox(), on_channel("Now playing: b"));
}

#[tokio::test]
async fn song_start_waits_until_the_song_is_heard() {
    let mut h = song_start(Event {
        message: text("Now playing: {song}"),
        ..Event::default()
    });
    h.add(&["a.mp3", "b.mp3"]).await;
    assert_eq!(h.outbox(), on_channel("Now playing: a"));
    let a = h.current_id();
    h.finish_current();
    assert!(h.outbox().is_empty());
    h.played(a);
    assert_eq!(h.outbox(), on_channel("Now playing: b"));
}

#[tokio::test]
async fn the_song_in_a_message_is_escaped() {
    let mut h = song_start(Event {
        message: text("[b]{song}[/b]"),
        ..Event::default()
    });
    fs::write(h.dir.path().join("[url=x]y[_url].mp3"), b"audio").unwrap();
    h.add(&["[url=x]y[_url].mp3"]).await;
    assert_eq!(h.outbox(), on_channel("[b]\\[url=x\\]y\\[_url\\][/b]"));
}

#[tokio::test]
async fn a_long_nickname_is_cut() {
    let mut h = song_start(Event {
        nickname: text("{default} | {song}"),
        ..Event::default()
    });
    let name = "a very long title of a song that goes on";
    fs::write(h.dir.path().join(format!("{name}.mp3")), b"audio").unwrap();
    h.add(&[&format!("{name}.mp3")]).await;
    assert_eq!(
        h.teamspeak_requests(),
        [TeamspeakRequest::SetNickname(
            "Music Bot | a very long title…".into()
        )]
    );
}

#[tokio::test]
async fn song_start_is_skipped_when_the_song_is_gone() {
    let mut h = song_start(Event {
        delay: Duration::from_secs(5),
        message: text("Now playing: {song}"),
        ..Event::default()
    });
    h.add(&["a.mp3"]).await;
    h.send("!queue clear");
    let later = tokio::time::Instant::now() + Duration::from_secs(6);
    let actions = h.bot.events.tick(later);
    assert_eq!(actions, [Action::Run(Trigger::SongStart)]);
    h.bot.run_actions(actions);
    assert!(h.teamspeak_requests().is_empty());
}

#[tokio::test]
async fn an_event_sound_is_not_a_song_start() {
    let mut h = Harness::with_config_in(|config, dir| {
        config.events.on_party.sound = Some(Sound::File(dir.join("hi.wav")));
        config.events.on_song_start.message = text("Now playing: {song}");
    });
    h.run(&[party()]);
    assert_eq!(h.sound(), Some("hi.wav"));
    assert!(h.teamspeak_requests().is_empty());
}

#[tokio::test]
async fn connecting_runs_on_connect() {
    let mut h = Harness::with_config(|config| {
        config.teamspeak.nickname = "Music Bot".into();
        config.events.on_connect = Event {
            message: text("Hello!"),
            nickname: text("{default}"),
            ..Event::default()
        };
    });
    h.connected();
    assert_eq!(
        h.teamspeak_requests(),
        [
            say("Hello!"),
            TeamspeakRequest::SetNickname("Music Bot".into())
        ]
    );
}

#[tokio::test]
async fn the_connect_sound_keeps_the_music_playing() {
    let mut h = Harness::with_config_in(|config, dir| {
        config.events.on_connect.sound = Some(Sound::File(dir.join("hi.wav")));
    });
    h.add(&["a.mp3", "b.mp3"]).await;
    h.connected();
    assert_eq!(h.playing(), Some("a"));
    assert_eq!(h.sound(), None);
}

#[tokio::test]
async fn songs_wait_for_the_connect_sound() {
    let mut h = Harness::with_config_in(|config, dir| {
        config.events.on_connect.sound = Some(Sound::File(dir.join("hi.wav")));
    });
    h.connected();
    assert_eq!(h.sound(), Some("hi.wav"));
    h.add(&["a.mp3"]).await;
    assert_eq!(h.queued(), ["a"]);
    h.sound_heard();
    assert_eq!(h.playing(), Some("a"));
}
