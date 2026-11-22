use crate::commands::{
    ADD_HELP, ADD_USAGE, BOT_HELP, CONFIG_HELP, Command, CommandError, GENERAL_HELP, GET_USAGE,
    LIST_USAGE, QUEUE_HELP, REMOVE_USAGE, SET_USAGE, SHOW_USAGE, SONG_HELP, VOLUME_USAGE,
};

use super::parse::{err, ok};

#[test]
fn bare_prefix_shows_help() {
    assert_eq!(ok("!song"), Command::Help(SONG_HELP));
    assert_eq!(ok("!queue"), Command::Help(QUEUE_HELP));
    assert_eq!(ok("!bot"), Command::Help(BOT_HELP));
    assert_eq!(ok("!bot config"), Command::Help(CONFIG_HELP));
}

#[test]
fn help_keyword_and_flag_show_help() {
    let cases = [
        ("!song", SONG_HELP),
        ("!queue", QUEUE_HELP),
        ("!bot", BOT_HELP),
        ("!bot config", CONFIG_HELP),
    ];
    for (prefix, help) in cases {
        assert_eq!(
            ok(&format!("{prefix} help")),
            Command::Help(help),
            "{prefix}"
        );
        assert_eq!(
            ok(&format!("{prefix} --help")),
            Command::Help(help),
            "{prefix}"
        );
    }
}

#[test]
fn add_help_flag_shows_add_help() {
    assert_eq!(ok("!song add --help"), Command::Help(ADD_HELP));
    assert_eq!(ok("!song add --album --help"), Command::Help(ADD_HELP));
    assert_eq!(
        ok("!song add --source youtube --help x"),
        Command::Help(ADD_HELP)
    );
}

#[test]
fn help_after_the_phrase_start_is_searched() {
    assert!(matches!(ok("!song add help"), Command::Add { .. }));
    assert!(matches!(
        ok("!song add the beatles --help"),
        Command::Add { .. }
    ));
}

#[test]
fn help_with_extra_words_is_not_help() {
    assert!(Command::parse("!song help me").unwrap().is_err());
    assert!(Command::parse("!queue --help now").unwrap().is_err());
}

#[test]
fn help_lists_the_same_usage_as_the_errors() {
    assert!(SONG_HELP.contains(ADD_USAGE));
    assert!(ADD_HELP.contains(ADD_USAGE));
    for usage in [SHOW_USAGE, REMOVE_USAGE] {
        assert!(QUEUE_HELP.contains(usage), "{usage}");
    }
    assert!(BOT_HELP.contains(VOLUME_USAGE));
    for usage in [LIST_USAGE, GET_USAGE, SET_USAGE] {
        assert!(CONFIG_HELP.contains(usage), "{usage}");
    }
}

#[test]
fn help_fits_one_chat_message() {
    for help in [
        GENERAL_HELP,
        SONG_HELP,
        ADD_HELP,
        QUEUE_HELP,
        BOT_HELP,
        CONFIG_HELP,
    ] {
        assert!(help.is_ascii(), "{help}");
        assert!(help.len() < 1024, "{help}");
        assert!(!help.starts_with('\n') && !help.ends_with('\n'), "{help}");
    }
}

#[test]
fn general_help_shows_where_to_start() {
    assert_eq!(ok("!help"), Command::Help(GENERAL_HELP));
    assert_eq!(ok("!help me please"), Command::Help(GENERAL_HELP));
    assert_eq!(ok("  !help  "), Command::Help(GENERAL_HELP));
}

#[test]
fn general_help_points_to_every_prefix_help() {
    for prefix in ["!song", "!queue", "!bot"] {
        assert!(GENERAL_HELP.contains(&format!("{prefix} help")), "{prefix}");
    }
}

#[test]
fn help_typo_gets_a_suggestion() {
    assert_eq!(err("!hlep"), CommandError::DidYouMean("!help"));
    assert_eq!(err("!Help"), CommandError::DidYouMean("!help"));
    assert_eq!(Command::parse("help"), None);
}
