use std::{fs, io, path::Path};

use super::harness::{Harness, on_channel};
use crate::{
    audio::{AudioError, AudioInput, Decoder},
    player::{Ended, chat_reason},
    sources::youtube::YoutubeError,
};

#[tokio::test]
async fn failed_track_is_reported_and_the_queue_moves_on() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    h.fail_current("this video is age-restricted");
    assert_eq!(
        h.outbox(),
        on_channel("could not play a: this video is age-restricted")
    );
    assert_eq!(h.playing(), Some("b"));
}

#[tokio::test]
async fn finished_track_sends_nothing() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    h.finish_current();
    assert!(h.outbox().is_empty());
}

#[tokio::test]
async fn failure_of_an_old_track_sends_nothing() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3"]).await;
    let old = h.current_id();
    assert_eq!(h.send("!song next"), None);
    h.bot.finished(Ended {
        id: old,
        failure: Some("playback failed".to_owned()),
        end_frame: true,
    });
    assert!(h.outbox().is_empty());
    assert_eq!(h.playing(), Some("b"));
}

#[tokio::test]
async fn failure_while_paused_is_reported() {
    let mut h = Harness::new();
    h.add(&["a.mp3"]).await;
    assert_eq!(h.send("!song pause"), None);
    h.fail_current("playback failed");
    assert_eq!(h.outbox(), on_channel("could not play a: playback failed"));
}

#[tokio::test]
async fn unopenable_track_is_reported() {
    let mut h = Harness::new();
    h.add(&["a.mp3", "b.mp3", "c.mp3"]).await;
    fs::remove_file(h.dir.path().join("b.mp3")).unwrap();
    h.finish_current();
    assert_eq!(
        h.outbox(),
        on_channel("could not play b: no such file in the music directory")
    );
    assert_eq!(h.playing(), Some("c"));
}

#[tokio::test]
async fn titles_in_messages_cannot_format_the_chat() {
    let mut h = Harness::new();
    fs::write(h.dir.path().join("[b]x.mp3"), b"audio").unwrap();
    h.add(&["[b]x.mp3"]).await;
    h.fail_current("playback failed");
    assert_eq!(
        h.outbox(),
        on_channel("could not play \\[b\\]x: playback failed")
    );
}

#[test]
fn only_known_source_messages_reach_the_chat() {
    let age = AudioError::Input(io::Error::other(YoutubeError::AgeRestricted));
    assert_eq!(chat_reason(&age), "this video is age-restricted");
    let other = AudioError::Input(io::Error::other("/home/user/secret path"));
    assert_eq!(chat_reason(&other), "playback failed");
    let ffmpeg = AudioError::Output(io::Error::other("ffmpeg said something"));
    assert_eq!(chat_reason(&ffmpeg), "playback failed");
}

struct AgeGatedStream;

impl io::Read for AgeGatedStream {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other(YoutubeError::AgeRestricted))
    }
}

#[test]
fn source_message_survives_the_decoder() {
    let input = AudioInput::Stream(Box::new(AgeGatedStream));
    let mut decoder = Decoder::spawn(Path::new("ffmpeg"), input).unwrap();
    while decoder.read_frame().unwrap().is_some() {}
    let err = decoder.finish().unwrap_err();
    assert_eq!(chat_reason(&err), "this video is age-restricted");
}
