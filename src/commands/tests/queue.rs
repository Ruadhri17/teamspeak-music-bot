use std::num::NonZeroUsize;

use super::parse::{err, ok};
use crate::commands::{Command, CommandError};

fn n(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).unwrap()
}

#[test]
fn show_defaults_to_first_page() {
    assert_eq!(ok("!queue show"), Command::QueueShow { page: n(1) });
}

#[test]
fn show_and_display_are_aliases() {
    assert_eq!(ok("!queue show 3"), Command::QueueShow { page: n(3) });
    assert_eq!(ok("!queue display 3"), Command::QueueShow { page: n(3) });
}

#[test]
fn show_rejects_bad_pages() {
    assert_eq!(
        err("!queue show 0"),
        CommandError::InvalidNumber("0".into())
    );
    assert_eq!(
        err("!queue show -1"),
        CommandError::InvalidNumber("-1".into())
    );
    assert_eq!(
        err("!queue show two"),
        CommandError::InvalidNumber("two".into())
    );
    assert_eq!(
        err("!queue show 1 2"),
        CommandError::Usage("!queue show|display [page]")
    );
}

#[test]
fn remove() {
    assert_eq!(
        ok("!queue remove 5"),
        Command::QueueRemove { position: n(5) }
    );
}

#[test]
fn remove_needs_one_number() {
    let usage = CommandError::Usage("!queue remove <number>");
    assert_eq!(err("!queue remove"), usage);
    assert_eq!(err("!queue remove 1 2"), usage);
    assert_eq!(
        err("!queue remove 0"),
        CommandError::InvalidNumber("0".into())
    );
}

#[test]
fn clear() {
    assert_eq!(ok("!queue clear"), Command::QueueClear);
}

#[test]
fn unknown_queue_command() {
    assert_eq!(
        err("!queue shuffle"),
        CommandError::UnknownCommand("!queue show|remove|clear|help")
    );
}
