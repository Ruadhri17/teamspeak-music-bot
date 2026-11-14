use crate::commands::{Command, CommandError};

pub fn ok(text: &str) -> Command {
    Command::parse(text).unwrap().unwrap()
}

pub fn err(text: &str) -> CommandError {
    Command::parse(text).unwrap().unwrap_err()
}

#[test]
fn normal_chat_is_not_a_command() {
    assert_eq!(Command::parse("hi, what are we playing?"), None);
    assert_eq!(Command::parse(""), None);
    assert_eq!(Command::parse("   "), None);
}

#[test]
fn other_prefixes_are_ignored() {
    assert_eq!(Command::parse("!help"), None);
    assert_eq!(Command::parse("!songs add x"), None);
    assert_eq!(Command::parse("song add x"), None);
}

#[test]
fn prefix_must_be_the_first_word() {
    assert_eq!(Command::parse("play it !song add x"), None);
}

#[test]
fn extra_whitespace_is_ignored() {
    assert_eq!(ok("  !song   pause  "), Command::Pause);
    assert_eq!(ok("!song\tnext"), Command::Next);
}

#[test]
fn bare_prefix_lists_its_commands() {
    assert_eq!(
        err("!song"),
        CommandError::UnknownCommand("!song add|pause|resume|next|current")
    );
    assert_eq!(
        err("!queue"),
        CommandError::UnknownCommand("!queue show|remove|clear")
    );
    assert_eq!(
        err("!bot"),
        CommandError::UnknownCommand("!bot summon|config")
    );
}

#[test]
fn error_messages_are_ascii_and_short() {
    let errors = [
        err("!song"),
        err("!song add"),
        err("!song add --sorce tidal x"),
        err("!song add --source"),
        err("!song add --source tidal --source youtube x"),
        err("!song add --album --playlist x"),
        err("!song add --source deezer x"),
        err("!queue remove abc"),
    ];
    for error in errors {
        let message = error.to_string();
        assert!(message.is_ascii(), "{message}");
        assert!(message.len() < 100, "{message}");
    }
}
