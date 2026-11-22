use super::parse::{err, ok};
use crate::commands::{Command, CommandError};

#[test]
fn pause_and_stop_are_aliases() {
    assert_eq!(ok("!song pause"), Command::Pause);
    assert_eq!(ok("!song stop"), Command::Pause);
}

#[test]
fn next_and_skip_are_aliases() {
    assert_eq!(ok("!song next"), Command::Next);
    assert_eq!(ok("!song skip"), Command::Next);
}

#[test]
fn resume_and_current() {
    assert_eq!(ok("!song resume"), Command::Resume);
    assert_eq!(ok("!song current"), Command::Current);
}

#[test]
fn unknown_song_command() {
    assert_eq!(
        err("!song dodaj x"),
        CommandError::UnknownCommand("!song add|pause|resume|next|current")
    );
}

#[test]
fn simple_commands_reject_extra_words() {
    assert!(matches!(
        err("!song pause now"),
        CommandError::UnknownCommand(_)
    ));
    assert!(matches!(
        err("!song current x"),
        CommandError::UnknownCommand(_)
    ));
}

#[test]
fn commands_are_case_sensitive() {
    assert!(matches!(
        err("!song PAUSE"),
        CommandError::UnknownCommand(_)
    ));
    assert_eq!(err("!Song pause"), CommandError::DidYouMean("!song"));
}
