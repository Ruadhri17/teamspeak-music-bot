use std::{
    fs::File,
    panic::{self, AssertUnwindSafe},
    path::PathBuf,
    process::Command,
    sync::mpsc as std_mpsc,
    time::Duration,
};

use tokio::sync::mpsc;

use crate::{
    audio::{AudioError, AudioInput, DoneGuard, Frame, PlayResult, play},
    process::Stopper,
};

const TIMEOUT: Duration = Duration::from_secs(20);

fn sine(dir: &tempfile::TempDir, seconds: u32) -> AudioInput {
    let path = dir.path().join("sine.wav");
    let status = Command::new("ffmpeg")
        .args(["-loglevel", "error", "-f", "lavfi", "-i"])
        .arg(format!("sine=frequency=440:duration={seconds}"))
        .arg(&path)
        .status()
        .expect("ffmpeg is installed");
    assert!(status.success());
    AudioInput::File(File::open(path).unwrap())
}

struct Run {
    frames: mpsc::Receiver<Frame>,
    done: std_mpsc::Receiver<PlayResult>,
    stop: Stopper,
}

fn start(ffmpeg: &str, input: AudioInput, capacity: usize) -> Run {
    let (frames_tx, frames) = mpsc::channel(capacity);
    let (done_tx, done) = std_mpsc::channel();
    let stop = Stopper::new();
    play(
        PathBuf::from(ffmpeg),
        input,
        7,
        frames_tx,
        stop.clone(),
        move |result| done_tx.send(result).unwrap(),
    );
    Run { frames, done, stop }
}

#[test]
fn whole_track_reports_ok_with_tagged_frames() {
    let dir = tempfile::tempdir().unwrap();
    let mut run = start("ffmpeg", sine(&dir, 1), 100);
    let result = run.done.recv_timeout(TIMEOUT).unwrap();
    assert!(result.is_ok(), "{result:?}");
    let mut count = 0;
    while let Ok(frame) = run.frames.try_recv() {
        assert_eq!(frame.stream, 7);
        assert!(frame.pcm.iter().any(|sample| sample.abs() > 0.01));
        count += 1;
    }
    assert!((49..=51).contains(&count), "{count} frames");
}

#[test]
fn missing_ffmpeg_reports_spawn_error() {
    let run = start(
        "/nonexistent/ffmpeg",
        AudioInput::Stream(Box::new(&b""[..])),
        8,
    );
    let result = run.done.recv_timeout(TIMEOUT).unwrap();
    assert!(matches!(result, Err(AudioError::Spawn(_))), "{result:?}");
}

#[test]
fn invalid_input_reports_ffmpeg_error() {
    let run = start(
        "ffmpeg",
        AudioInput::Stream(Box::new(&b"this is not audio"[..])),
        8,
    );
    let result = run.done.recv_timeout(TIMEOUT).unwrap();
    assert!(
        matches!(result, Err(AudioError::Ffmpeg { .. })),
        "{result:?}"
    );
}

#[test]
fn stop_reports_ok() {
    let dir = tempfile::tempdir().unwrap();
    let mut run = start("ffmpeg", sine(&dir, 30), 1);
    run.frames.blocking_recv().unwrap();
    run.stop.stop();
    run.frames.blocking_recv();
    let result = run.done.recv_timeout(TIMEOUT).unwrap();
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn closed_receiver_reports_ok() {
    let dir = tempfile::tempdir().unwrap();
    let run = start("ffmpeg", sine(&dir, 30), 1);
    drop(run.frames);
    let result = run.done.recv_timeout(TIMEOUT).unwrap();
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn guard_reports_panicked_when_dropped_by_a_panic() {
    let (done_tx, done) = std_mpsc::channel();
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let _guard = DoneGuard(Some(move |result| done_tx.send(result).unwrap()));
        panic!("decoder bug");
    }));
    assert!(result.is_err());
    assert!(matches!(done.try_recv(), Ok(Err(AudioError::Panicked))));
    assert!(done.try_recv().is_err());
}

#[test]
fn guard_finish_reports_once() {
    let (done_tx, done) = std_mpsc::channel();
    let guard = DoneGuard(Some(move |result| done_tx.send(result).unwrap()));
    guard.finish(Ok(()));
    assert!(matches!(done.try_recv(), Ok(Ok(()))));
    assert!(done.try_recv().is_err());
}

#[test]
fn stop_frees_a_playback_whose_input_sends_nothing() {
    let (reader, writer) = std::os::unix::net::UnixStream::pair().unwrap();
    let run = start("ffmpeg", AudioInput::Stream(Box::new(reader)), 1);
    std::thread::sleep(Duration::from_millis(200));
    let start = std::time::Instant::now();
    run.stop.stop();
    let result = run
        .done
        .recv_timeout(Duration::from_secs(3))
        .expect("the stopped playback ends at once");
    assert!(result.is_ok(), "{result:?}");
    assert!(start.elapsed() < Duration::from_secs(2));
    drop(writer);
}
