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
    for text in [
        "!play x",
        "!stop",
        "!skip",
        "!list",
        "!next",
        "!vol 50",
        "!so",
        "!b",
        "!",
        "song add x",
        "sond add x",
    ] {
        assert_eq!(Command::parse(text), None, "{text}");
    }
}

#[test]
fn prefix_with_a_typo_gets_a_suggestion() {
    let cases = [
        ("!sond add x", "!song"),
        ("!son", "!song"),
        ("!songs add x", "!song"),
        ("!snog", "!song"),
        ("!Song add x", "!song"),
        ("!SONG", "!song"),
        ("!qeue show", "!queue"),
        ("!queu", "!queue"),
        ("!bto summon", "!bot"),
        ("!boot", "!bot"),
    ];
    for (text, prefix) in cases {
        assert_eq!(err(text), CommandError::DidYouMean(prefix), "{text}");
    }
}

#[test]
fn suggestion_never_repeats_the_typed_text() {
    let message = err("!sond add <script>").to_string();
    assert_eq!(message, "unknown command, did you mean !song?");
}

#[test]
fn multi_byte_typo_counts_as_one_character() {
    assert_eq!(err("!sóng"), CommandError::DidYouMean("!song"));
    assert_eq!(Command::parse("!sóńg"), None);
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
fn error_messages_are_ascii_and_short() {
    let errors = [
        err("!song dance"),
        err("!song add"),
        err("!song add --sorce tidal x"),
        err("!song add --source"),
        err("!song add --source tidal --source youtube x"),
        err("!song add --album --playlist x"),
        err("!song add --source deezer x"),
        err("!queue remove abc"),
        err("!sond add x"),
    ];
    for error in errors {
        let message = error.to_string();
        assert!(message.is_ascii(), "{message}");
        assert!(message.len() < 100, "{message}");
    }
}
