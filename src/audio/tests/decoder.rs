use std::{fs::File, path::Path, process::Command};

use tempfile::TempDir;

use crate::audio::{AudioError, AudioInput, FRAME_SAMPLES, decoder::Decoder};

fn sine(dir: &TempDir, seconds: u32) -> AudioInput {
    let path = dir.path().join("sine.wav");
    let status = Command::new("ffmpeg")
        .args(["-loglevel", "error", "-f", "lavfi", "-i"])
        .arg(format!("sine=frequency=440:duration={seconds}"))
        .arg(&path)
        .status()
        .expect("ffmpeg is installed");
    assert!(status.success());
    Box::new(File::open(path).unwrap())
}

fn count_frames(decoder: &mut Decoder) -> usize {
    std::iter::from_fn(|| decoder.read_frame().unwrap()).count()
}

#[test]
fn decodes_two_seconds_into_about_a_hundred_frames() {
    let dir = tempfile::tempdir().unwrap();
    let mut decoder = Decoder::spawn(Path::new("ffmpeg"), sine(&dir, 2)).unwrap();
    let frames = count_frames(&mut decoder);
    assert!((99..=101).contains(&frames), "{frames} frames");
}

#[test]
fn frames_contain_sound() {
    let dir = tempfile::tempdir().unwrap();
    let mut decoder = Decoder::spawn(Path::new("ffmpeg"), sine(&dir, 1)).unwrap();
    let frame = decoder.read_frame().unwrap().unwrap();
    assert_eq!(frame.len(), FRAME_SAMPLES);
    assert!(frame.iter().any(|sample| sample.abs() > 0.01));
    assert!(frame.iter().all(|sample| sample.abs() <= 1.0));
}

#[test]
fn invalid_input_gives_no_frames_and_an_ffmpeg_error() {
    let input: AudioInput = Box::new(&b"this is not audio"[..]);
    let mut decoder = Decoder::spawn(Path::new("ffmpeg"), input).unwrap();
    assert_eq!(count_frames(&mut decoder), 0);
    match decoder.finish() {
        Err(AudioError::Ffmpeg { status, stderr }) => {
            assert!(!status.success());
            assert!(!stderr.is_empty());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn whole_track_finishes_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let mut decoder = Decoder::spawn(Path::new("ffmpeg"), sine(&dir, 1)).unwrap();
    count_frames(&mut decoder);
    assert!(decoder.finish().is_ok());
}

#[test]
fn input_read_error_is_reported() {
    let input: AudioInput = Box::new(FailingReader);
    let mut decoder = Decoder::spawn(Path::new("ffmpeg"), input).unwrap();
    count_frames(&mut decoder);
    assert!(matches!(decoder.finish(), Err(AudioError::Input(_))));
}

struct FailingReader;

impl std::io::Read for FailingReader {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("connection reset"))
    }
}

#[test]
fn dropping_mid_track_stops_ffmpeg() {
    let dir = tempfile::tempdir().unwrap();
    let mut decoder = Decoder::spawn(Path::new("ffmpeg"), sine(&dir, 30)).unwrap();
    decoder.read_frame().unwrap().unwrap();
    drop(decoder);
}

#[test]
fn missing_ffmpeg_is_an_error() {
    let input: AudioInput = Box::new(&b""[..]);
    assert!(Decoder::spawn(Path::new("/nonexistent/ffmpeg"), input).is_err());
}
