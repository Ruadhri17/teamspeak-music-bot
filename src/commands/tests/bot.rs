use super::parse::{err, ok};
use crate::commands::{Command, CommandError};

#[test]
fn ping_and_summon() {
    assert_eq!(ok("!bot ping"), Command::Ping);
    assert_eq!(ok("!bot summon"), Command::Summon);
}

#[test]
fn config_list() {
    assert_eq!(
        ok("!bot config list"),
        Command::ConfigList { section: None }
    );
    assert_eq!(
        ok("!bot config list audio"),
        Command::ConfigList {
            section: Some("audio".into())
        }
    );
    assert_eq!(
        err("!bot config list audio paths"),
        CommandError::Usage("!bot config list [section]")
    );
}

#[test]
fn config_get() {
    assert_eq!(
        ok("!bot config get audio.bitrate"),
        Command::ConfigGet {
            key: "audio.bitrate".into()
        }
    );
    assert_eq!(
        err("!bot config get"),
        CommandError::Usage("!bot config get <section.key>")
    );
}

#[test]
fn config_set_keeps_spaces_in_value() {
    assert_eq!(
        ok("!bot config set teamspeak.description drag me to your channel"),
        Command::ConfigSet {
            key: "teamspeak.description".into(),
            value: "drag me to your channel".into()
        }
    );
}

#[test]
fn config_set_needs_key_and_value() {
    let usage = CommandError::Usage("!bot config set <section.key> <value>");
    assert_eq!(err("!bot config set"), usage);
    assert_eq!(err("!bot config set audio.bitrate"), usage);
}

#[test]
fn config_reload() {
    assert_eq!(ok("!bot config reload"), Command::ConfigReload);
}

#[test]
fn unknown_bot_commands() {
    assert_eq!(
        err("!bot dance"),
        CommandError::UnknownCommand("!bot summon|ping|config|help")
    );
    assert_eq!(
        err("!bot config delete x"),
        CommandError::UnknownCommand("!bot config list|get|set|reload|help")
    );
}
