use tokio::sync::mpsc;

use crate::{
    audio::{FRAME_SAMPLES, Frame},
    teamspeak::frames::{Frames, Pcm},
};

fn audio(stream: u64, n: u16) -> Frame {
    Frame::Audio {
        stream,
        pcm: Box::new([f32::from(n); FRAME_SAMPLES]),
    }
}

fn end(stream: u64) -> Frame {
    Frame::End { stream }
}

fn frames(start: usize, queued: Vec<Frame>) -> (Frames, mpsc::Sender<Frame>) {
    let (tx, rx) = mpsc::channel(64);
    for frame in queued {
        tx.try_send(frame).unwrap();
    }
    (Frames::new(rx, start), tx)
}

fn n(pcm: Option<Pcm>) -> Option<u16> {
    pcm.map(|pcm| pcm[0] as u16)
}

#[test]
fn silence_waits_for_the_start_buffer() {
    let (mut frames, tx) = frames(3, vec![audio(1, 1), audio(1, 2)]);
    assert_eq!(n(frames.next(0, false)), None);
    tx.try_send(audio(1, 3)).unwrap();
    assert_eq!(n(frames.next(0, false)), Some(1));
    assert_eq!(n(frames.next(0, true)), Some(2));
    assert_eq!(n(frames.next(0, true)), Some(3));
}

#[test]
fn while_speaking_the_next_frame_goes_out_at_once() {
    let (mut frames, _tx) = frames(5, vec![audio(1, 1)]);
    assert_eq!(n(frames.next(0, true)), Some(1));
}

#[test]
fn a_stream_shorter_than_the_start_buffer_starts_once_its_end_is_in() {
    let (mut frames, _tx) = frames(10, vec![audio(1, 1), end(1)]);
    assert_eq!(n(frames.next(0, false)), Some(1));
    assert!(frames.take_ended().is_empty());
    assert_eq!(n(frames.next(0, true)), None);
    assert_eq!(frames.take_ended(), [1]);
}

#[test]
fn the_next_stream_follows_its_end_in_the_same_tick() {
    let (mut frames, _tx) = frames(1, vec![audio(1, 1), end(1), audio(2, 7)]);
    assert_eq!(n(frames.next(0, false)), Some(1));
    assert_eq!(n(frames.next(0, true)), Some(7));
    assert_eq!(frames.take_ended(), [1]);
    assert!(frames.take_ended().is_empty());
}

#[test]
fn stopped_streams_are_dropped_with_their_end() {
    let (mut frames, _tx) = frames(1, vec![audio(1, 1), end(1), audio(2, 5)]);
    assert_eq!(n(frames.next(2, false)), Some(5));
    assert!(frames.take_ended().is_empty());
}

#[test]
fn a_stop_drops_frames_gathered_for_the_start() {
    let (mut frames, tx) = frames(3, vec![audio(1, 1), audio(1, 2)]);
    assert_eq!(n(frames.next(0, false)), None);
    tx.try_send(audio(2, 9)).unwrap();
    tx.try_send(end(2)).unwrap();
    assert_eq!(n(frames.next(2, false)), Some(9));
}

#[test]
fn after_running_dry_the_start_buffer_fills_again() {
    let (mut frames, tx) = frames(2, vec![audio(1, 1), audio(1, 2)]);
    assert_eq!(n(frames.next(0, false)), Some(1));
    assert_eq!(n(frames.next(0, true)), Some(2));
    assert_eq!(n(frames.next(0, true)), None);
    tx.try_send(audio(1, 3)).unwrap();
    assert_eq!(n(frames.next(0, false)), None);
    tx.try_send(audio(1, 4)).unwrap();
    assert_eq!(n(frames.next(0, false)), Some(3));
}

#[test]
fn a_pause_keeps_one_frame_and_frees_the_channel_of_stopped_streams() {
    let (mut frames, tx) = frames(1, vec![audio(1, 1), audio(1, 2), audio(2, 3), audio(2, 4)]);
    frames.hold(2);
    assert_eq!(tx.capacity(), 63);
    frames.hold(2);
    assert_eq!(tx.capacity(), 63);
    assert_eq!(n(frames.next(2, false)), Some(3));
    assert_eq!(n(frames.next(2, true)), Some(4));
}

#[test]
fn an_empty_channel_gives_nothing() {
    let (mut frames, _tx) = frames(1, Vec::new());
    assert_eq!(n(frames.next(0, false)), None);
    assert_eq!(n(frames.next(0, true)), None);
    frames.hold(0);
    assert!(frames.take_ended().is_empty());
}
