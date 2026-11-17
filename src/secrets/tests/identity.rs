use std::{fs, os::unix::fs::PermissionsExt, path::Path};

use crate::secrets::{
    file::FileError,
    identity::{self, IdentityError},
};

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn first_start_creates_a_private_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state").join("identity.toml");
    identity::load_or_create(&path).unwrap();
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(&dir.path().join("state")), 0o700);
}

#[test]
fn second_start_keeps_the_uid() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity.toml");
    let first = identity::load_or_create(&path).unwrap();
    let second = identity::load_or_create(&path).unwrap();
    assert_eq!(identity::uid(&first), identity::uid(&second));
    assert_eq!(first.counter(), second.counter());
}

#[test]
fn new_identity_has_the_default_security_level() {
    let dir = tempfile::tempdir().unwrap();
    let identity = identity::load_or_create(&dir.path().join("identity.toml")).unwrap();
    assert!(identity.level() >= 8);
}

#[test]
fn saved_identity_survives_a_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity.toml");
    let mut identity = identity::load_or_create(&path).unwrap();
    identity.upgrade_level(10);
    identity::save(&path, &identity).unwrap();
    let loaded = identity::load_or_create(&path).unwrap();
    assert_eq!(loaded.counter(), identity.counter());
    assert!(loaded.level() >= 10);
}

#[test]
fn uid_is_base64_of_a_sha1() {
    let dir = tempfile::tempdir().unwrap();
    let identity = identity::load_or_create(&dir.path().join("identity.toml")).unwrap();
    let uid = identity::uid(&identity);
    assert_eq!(uid.len(), 28, "{uid}");
    assert!(uid.ends_with('='), "{uid}");
}

#[test]
fn broken_file_is_an_error_and_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity.toml");
    let secret = "MEECAQAwEwYHKoZIzj0CAQYIKoZIzj0DAQcEJzAlAgEBBCB-broken";
    fs::write(&path, format!("key = \"{secret}\"\ncounter = 1\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let err = identity::load_or_create(&path).unwrap_err();
    assert!(matches!(err, IdentityError::Parse { .. }), "{err:?}");
    assert!(!format!("{err} {err:?}").contains(secret));
    assert!(fs::read_to_string(&path).unwrap().contains(secret));
}

#[test]
fn file_readable_by_others_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity.toml");
    identity::load_or_create(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        identity::load_or_create(&path),
        Err(IdentityError::File(FileError::Permissions { .. }))
    ));
}
