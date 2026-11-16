use std::{fs, io::Read, path::Path};

use tempfile::TempDir;

use crate::sources::{
    Location, TrackRef,
    local::{self, LocalError},
};

fn music_dir() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("song.mp3"), b"audio").unwrap();
    fs::create_dir(dir.path().join("album")).unwrap();
    fs::write(dir.path().join("album").join("track one.flac"), b"flac").unwrap();
    dir
}

fn not_found(result: Result<TrackRef, LocalError>) -> bool {
    matches!(result, Err(LocalError::NotFound))
}

#[test]
fn resolve_gives_relative_path_and_title() {
    let dir = music_dir();
    let track = local::resolve(dir.path(), "song.mp3").unwrap();
    assert_eq!(track.location, Location::Local("song.mp3".into()));
    assert_eq!(track.title, "song");
}

#[test]
fn resolve_works_in_subdirectories() {
    let dir = music_dir();
    let track = local::resolve(dir.path(), "album/track one.flac").unwrap();
    assert_eq!(
        track.location,
        Location::Local("album/track one.flac".into())
    );
    assert_eq!(track.title, "track one");
}

#[test]
fn resolve_normalizes_dots_inside_music_dir() {
    let dir = music_dir();
    let track = local::resolve(dir.path(), "album/../song.mp3").unwrap();
    assert_eq!(track.location, Location::Local("song.mp3".into()));
}

#[test]
fn missing_file_is_not_found() {
    let dir = music_dir();
    assert!(not_found(local::resolve(dir.path(), "nope.mp3")));
}

#[test]
fn directory_is_not_found() {
    let dir = music_dir();
    assert!(not_found(local::resolve(dir.path(), "album")));
    assert!(not_found(local::resolve(dir.path(), "")));
}

#[test]
fn parent_directory_is_not_found() {
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.txt"), b"x").unwrap();
    let dir = outside.path().join("music");
    fs::create_dir(&dir).unwrap();
    assert!(not_found(local::resolve(&dir, "../secret.txt")));
}

#[test]
fn absolute_path_outside_is_not_found() {
    let dir = music_dir();
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret.txt");
    fs::write(&secret, b"x").unwrap();
    assert!(not_found(local::resolve(
        dir.path(),
        secret.to_str().unwrap()
    )));
}

#[test]
fn symlink_leaving_music_dir_is_not_found() {
    let dir = music_dir();
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret.mp3");
    fs::write(&secret, b"x").unwrap();
    std::os::unix::fs::symlink(&secret, dir.path().join("link.mp3")).unwrap();
    assert!(not_found(local::resolve(dir.path(), "link.mp3")));
}

#[test]
fn open_reads_a_resolved_track() {
    let dir = music_dir();
    let Location::Local(path) = local::resolve(dir.path(), "song.mp3").unwrap().location;
    let mut data = Vec::new();
    local::open(dir.path(), &path)
        .unwrap()
        .read_to_end(&mut data)
        .unwrap();
    assert_eq!(data, b"audio");
}

#[test]
fn open_checks_the_path_again() {
    let dir = music_dir();
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret.mp3");
    fs::write(&secret, b"x").unwrap();
    fs::remove_file(dir.path().join("song.mp3")).unwrap();
    std::os::unix::fs::symlink(&secret, dir.path().join("song.mp3")).unwrap();
    assert!(matches!(
        local::open(dir.path(), Path::new("song.mp3")),
        Err(LocalError::NotFound)
    ));
}
