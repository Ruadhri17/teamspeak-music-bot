use std::{fs, os::unix::fs::PermissionsExt, path::Path};

use crate::secrets::file::{FileError, read_private, write_private};

fn write_with_mode(path: &Path, contents: &str, mode: u32) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn missing_file_is_none() {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        read_private(&dir.path().join("nope.toml"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn private_file_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.toml");
    write_with_mode(&path, "key = 1\n", 0o600);
    assert_eq!(read_private(&path).unwrap().as_deref(), Some("key = 1\n"));
}

#[test]
fn owner_read_only_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.toml");
    write_with_mode(&path, "x", 0o400);
    assert!(read_private(&path).unwrap().is_some());
}

#[test]
fn file_readable_by_others_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.toml");
    for mode in [0o644, 0o640, 0o604, 0o620] {
        write_with_mode(&path, "x", mode);
        let err = read_private(&path).unwrap_err();
        assert!(
            matches!(err, FileError::Permissions { mode: m, .. } if m == mode),
            "{mode:o}"
        );
    }
}

#[test]
fn permissions_message_has_the_fix() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.toml");
    write_with_mode(&path, "x", 0o644);
    let message = read_private(&path).unwrap_err().to_string();
    assert!(message.contains("mode 644"), "{message}");
    assert!(message.contains("chmod 600"), "{message}");
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn write_private_creates_a_private_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity.toml");
    write_private(&path, "key = 1\n").unwrap();
    assert_eq!(mode(&path), 0o600);
    assert_eq!(read_private(&path).unwrap().as_deref(), Some("key = 1\n"));
}

#[test]
fn write_private_creates_missing_directories_as_private() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let path = state.join("nested").join("identity.toml");
    write_private(&path, "x").unwrap();
    assert_eq!(mode(&state), 0o700);
    assert_eq!(mode(&state.join("nested")), 0o700);
    assert_eq!(mode(&path), 0o600);
}

#[test]
fn write_private_leaves_existing_directories_alone() {
    let dir = tempfile::tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
    write_private(&dir.path().join("identity.toml"), "x").unwrap();
    assert_eq!(mode(dir.path()), 0o755);
}

#[test]
fn write_private_replaces_a_file_and_tightens_its_mode() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity.toml");
    write_with_mode(&path, "old", 0o644);
    write_private(&path, "new").unwrap();
    assert_eq!(mode(&path), 0o600);
    assert_eq!(fs::read_to_string(&path).unwrap(), "new");
}

#[test]
fn write_private_replaces_a_stale_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity.toml");
    write_with_mode(&dir.path().join("identity.toml.tmp"), "stale", 0o644);
    write_private(&path, "new").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "new");
    assert!(!dir.path().join("identity.toml.tmp").exists());
}

#[test]
fn write_private_rejects_a_path_without_file_name() {
    let dir = tempfile::tempdir().unwrap();
    let err = write_private(&dir.path().join(".."), "x").unwrap_err();
    assert!(matches!(err, FileError::Write { .. }));
}
