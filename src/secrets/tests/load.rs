use std::{fs, os::unix::fs::PermissionsExt, path::Path};

use secrecy::ExposeSecret;
use tempfile::TempDir;

use crate::secrets::{Secrets, SecretsError};

const SECRET: &str = "hunter2-very-secret";

fn private_file(contents: &str) -> (TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secrets.toml");
    fs::write(&path, contents).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    (dir, path)
}

fn load(path: &Path) -> Result<Secrets, SecretsError> {
    Secrets::load_from(None, path)
}

fn assert_no_secret(err: &SecretsError) {
    assert!(!format!("{err}").contains(SECRET), "{err}");
    assert!(!format!("{err:?}").contains(SECRET), "{err:?}");
}

#[test]
fn missing_file_gives_empty_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = load(&dir.path().join("secrets.toml")).unwrap();
    assert!(secrets.teamspeak.server_password.is_none());
    assert!(secrets.tidal.client_id.is_none());
}

#[test]
fn values_are_read() {
    let (_dir, path) = private_file(&format!(
        "[teamspeak]\nserver_password = \"{SECRET}\"\n\n[tidal]\nclient_id = \"id\"\n"
    ));
    let secrets = load(&path).unwrap();
    assert_eq!(
        secrets.teamspeak.server_password.unwrap().expose_secret(),
        SECRET
    );
    assert_eq!(secrets.tidal.client_id.unwrap().expose_secret(), "id");
    assert!(secrets.tidal.client_secret.is_none());
}

#[test]
fn debug_does_not_show_values() {
    let (_dir, path) = private_file(&format!("[tidal]\nclient_secret = \"{SECRET}\"\n"));
    let secrets = load(&path).unwrap();
    assert!(!format!("{secrets:?}").contains(SECRET));
}

#[test]
fn file_readable_by_others_is_refused() {
    let (_dir, path) = private_file("");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(load(&path), Err(SecretsError::File(_))));
}

#[test]
fn syntax_error_reports_the_line_without_the_value() {
    let (_dir, path) = private_file(&format!(
        "[teamspeak]\nchannel_password = \"x\"\nserver_password = \"{SECRET}\n"
    ));
    let err = load(&path).unwrap_err();
    assert!(format!("{err}").contains("line 3"), "{err}");
    assert_no_secret(&err);
}

#[test]
fn wrong_type_does_not_show_the_value() {
    let (_dir, path) = private_file("[tidal]\nclient_id = 987654321\n");
    let err = load(&path).unwrap_err();
    assert!(format!("{err}").contains("line 2"), "{err}");
    assert!(!format!("{err:?}").contains("987654321"), "{err:?}");
}

#[test]
fn unknown_key_is_an_error() {
    let (_dir, path) = private_file(&format!("[tidal]\nclient_secert = \"{SECRET}\"\n"));
    let err = load(&path).unwrap_err();
    assert!(matches!(err, SecretsError::Parse { .. }));
    assert_no_secret(&err);
}

#[test]
fn empty_value_is_an_error() {
    let (_dir, path) =
        private_file("[teamspeak]\nserver_password = \"x\"\n\n[tidal]\nclient_id = \"  \"\n");
    let err = load(&path).unwrap_err();
    assert_eq!(
        format!("{err}"),
        format!(
            "invalid secrets file {}, line 5: must not be empty, remove the line instead",
            path.display()
        )
    );
}

#[test]
fn missing_values_are_fine() {
    let (_dir, path) = private_file("[teamspeak]\n\n[tidal]\nclient_id = \"id\"\n");
    let secrets = load(&path).unwrap();
    assert!(secrets.teamspeak.server_password.is_none());
    assert!(secrets.tidal.client_secret.is_none());
}

#[test]
fn systemd_credential_is_read_without_permission_check() {
    let dir = tempfile::tempdir().unwrap();
    let credential = dir.path().join("secrets");
    fs::write(&credential, "[tidal]\nclient_id = \"from-systemd\"\n").unwrap();
    fs::set_permissions(&credential, fs::Permissions::from_mode(0o444)).unwrap();
    let secrets = Secrets::load_from(Some(dir.path()), Path::new("/nonexistent")).unwrap();
    assert_eq!(
        secrets.tidal.client_id.unwrap().expose_secret(),
        "from-systemd"
    );
}

#[test]
fn missing_systemd_credential_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let err = Secrets::load_from(Some(dir.path()), Path::new("/nonexistent")).unwrap_err();
    assert!(matches!(err, SecretsError::Credential { .. }));
}
