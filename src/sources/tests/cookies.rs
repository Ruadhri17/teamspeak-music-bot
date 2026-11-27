use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use tempfile::TempDir;

use crate::{
    secrets::file::FileError,
    sources::cookies::{CookieCopy, CookiesError, check},
};

const COOKIES: &str = "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t0\tSID\tsecret\n";

fn cookie_file(mode: u32) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("youtube-cookies.txt");
    fs::write(&path, COOKIES).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
    (dir, path)
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn a_private_file_passes_the_check() {
    let (_dir, path) = cookie_file(0o600);
    check(&path).unwrap();
}

#[test]
fn a_missing_file_fails_the_check() {
    let dir = tempfile::tempdir().unwrap();
    let result = check(&dir.path().join("missing.txt"));
    assert!(
        matches!(result, Err(CookiesError::Missing { .. })),
        "{result:?}"
    );
}

#[test]
fn a_file_readable_by_others_fails_the_check() {
    let (_dir, path) = cookie_file(0o644);
    let result = check(&path);
    assert!(
        matches!(
            result,
            Err(CookiesError::File(FileError::Permissions { .. }))
        ),
        "{result:?}"
    );
}

#[test]
fn errors_do_not_contain_the_cookies() {
    let (_dir, path) = cookie_file(0o644);
    let err = anyhow::Error::from(check(&path).unwrap_err());
    assert!(!format!("{err:#}").contains("secret"));
}

#[test]
fn the_copy_has_the_same_cookies() {
    let (_dir, path) = cookie_file(0o600);
    let copy = CookieCopy::new(&path).unwrap();
    assert_ne!(copy.path(), path);
    assert_eq!(fs::read_to_string(copy.path()).unwrap(), COOKIES);
}

#[test]
fn only_the_bot_can_open_the_copy() {
    let (_dir, path) = cookie_file(0o600);
    let copy = CookieCopy::new(&path).unwrap();
    assert_eq!(mode(&copy.path()), 0o600);
    assert_eq!(mode(copy.path().parent().unwrap()), 0o700);
}

#[test]
fn every_copy_is_its_own_file() {
    let (_dir, path) = cookie_file(0o600);
    let first = CookieCopy::new(&path).unwrap();
    let second = CookieCopy::new(&path).unwrap();
    assert_ne!(first.path(), second.path());
}

#[test]
fn dropping_the_copy_removes_its_directory() {
    let (_dir, path) = cookie_file(0o600);
    let copy = CookieCopy::new(&path).unwrap();
    let copied = copy.path();
    drop(copy);
    assert!(!copied.exists());
    assert!(!copied.parent().unwrap().exists());
}

#[test]
fn writing_the_copy_leaves_the_file_as_it_was() {
    let (_dir, path) = cookie_file(0o600);
    let copy = CookieCopy::new(&path).unwrap();
    fs::write(copy.path(), "rewritten").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), COOKIES);
}

#[test]
fn every_copy_reads_the_file_again() {
    let (_dir, path) = cookie_file(0o600);
    fs::write(&path, "new export").unwrap();
    let copy = CookieCopy::new(&path).unwrap();
    assert_eq!(fs::read_to_string(copy.path()).unwrap(), "new export");
}

#[test]
fn an_unprotected_file_is_not_copied() {
    let (_dir, path) = cookie_file(0o640);
    assert!(matches!(
        CookieCopy::new(&path),
        Err(CookiesError::File(FileError::Permissions { .. }))
    ));
}
