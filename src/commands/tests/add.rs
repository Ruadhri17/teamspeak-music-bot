use super::parse::{err, ok};
use crate::{
    commands::{Command, CommandError},
    config::SourceKind,
    sources::SearchKind,
};

fn add(query: &str, source: Option<SourceKind>, kind: SearchKind) -> Command {
    Command::Add {
        query: query.into(),
        source,
        kind,
    }
}

#[test]
fn phrase_without_flags() {
    assert_eq!(
        ok("!song add daft punk one more time"),
        add("daft punk one more time", None, SearchKind::Track)
    );
}

#[test]
fn single_word_query() {
    assert_eq!(
        ok("!song add https://tidal.com/browse/track/123"),
        add(
            "https://tidal.com/browse/track/123",
            None,
            SearchKind::Track
        )
    );
}

#[test]
fn path_with_spaces_is_kept_whole() {
    assert_eq!(
        ok("!song add /mnt/c/Users/Some User/a b.mp3"),
        add("/mnt/c/Users/Some User/a b.mp3", None, SearchKind::Track)
    );
}

#[test]
fn source_flag() {
    assert_eq!(
        ok("!song add --source youtube lofi"),
        add("lofi", Some(SourceKind::Youtube), SearchKind::Track)
    );
}

#[test]
fn every_source_name() {
    let names = [
        ("tidal", SourceKind::Tidal),
        ("spotify", SourceKind::Spotify),
        ("youtube", SourceKind::Youtube),
        ("soundcloud", SourceKind::Soundcloud),
        ("local", SourceKind::Local),
    ];
    for (name, kind) in names {
        let command = ok(&format!("!song add --source {name} x"));
        assert_eq!(command, add("x", Some(kind), SearchKind::Track));
    }
}

#[test]
fn source_name_is_case_insensitive() {
    assert_eq!(
        ok("!song add --source Tidal x"),
        add("x", Some(SourceKind::Tidal), SearchKind::Track)
    );
}

#[test]
fn every_kind_flag() {
    assert_eq!(ok("!song add --track x"), add("x", None, SearchKind::Track));
    assert_eq!(ok("!song add --album x"), add("x", None, SearchKind::Album));
    assert_eq!(
        ok("!song add --playlist x"),
        add("x", None, SearchKind::Playlist)
    );
}

#[test]
fn flags_in_any_order() {
    let expected = add("discovery", Some(SourceKind::Tidal), SearchKind::Album);
    assert_eq!(ok("!song add --album --source tidal discovery"), expected);
    assert_eq!(ok("!song add --source tidal --album discovery"), expected);
}

#[test]
fn flags_after_the_phrase_are_part_of_it() {
    assert_eq!(
        ok("!song add daft punk --album"),
        add("daft punk --album", None, SearchKind::Track)
    );
}

#[test]
fn missing_query() {
    let usage = CommandError::Usage(
        "!song add [--track|--album|--playlist] [--source <name>] <link | phrase>",
    );
    assert_eq!(err("!song add"), usage);
    assert_eq!(err("!song add --album"), usage);
    assert_eq!(err("!song add --source tidal"), usage);
}

#[test]
fn source_without_value() {
    assert_eq!(
        err("!song add --source"),
        CommandError::MissingValue("--source")
    );
}

#[test]
fn source_followed_by_flag_is_unknown_source() {
    assert_eq!(
        err("!song add --source --album x"),
        CommandError::UnknownSource("--album".into())
    );
}

#[test]
fn unknown_source() {
    assert_eq!(
        err("!song add --source deezer x"),
        CommandError::UnknownSource("deezer".into())
    );
}

#[test]
fn unknown_flag() {
    assert_eq!(
        err("!song add --sorce tidal x"),
        CommandError::UnknownFlag("--sorce".into())
    );
}

#[test]
fn repeated_source() {
    assert_eq!(
        err("!song add --source tidal --source youtube x"),
        CommandError::RepeatedFlag("--source")
    );
}

#[test]
fn conflicting_kinds() {
    assert_eq!(
        err("!song add --album --playlist x"),
        CommandError::ConflictingKinds
    );
    assert_eq!(
        err("!song add --track --track x"),
        CommandError::ConflictingKinds
    );
}
