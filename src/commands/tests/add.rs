use url::Url;

use super::parse::{err, ok};
use crate::{
    commands::{Command, CommandError},
    config::SourceKind,
    sources::{QueryType, SearchKind},
};

fn add(query: &str, source: Option<SourceKind>, kind: SearchKind) -> Command {
    Command::Add {
        query: QueryType::Phrase(query.to_owned()),
        source,
        kind,
    }
}

fn add_link(link: &str) -> Command {
    Command::Add {
        query: QueryType::Link(Url::parse(link).unwrap()),
        source: None,
        kind: SearchKind::Track,
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
fn link_becomes_a_link() {
    assert_eq!(
        ok("!song add https://tidal.com/browse/track/123"),
        add_link("https://tidal.com/browse/track/123")
    );
}

#[test]
fn schemeless_youtube_link_becomes_a_link() {
    assert_eq!(
        ok("!song add youtu.be/5NV6Rdv1a3I"),
        add_link("https://youtu.be/5NV6Rdv1a3I")
    );
}

#[test]
fn ts3_url_tag_is_removed() {
    assert_eq!(
        ok("!song add [URL]https://www.youtube.com/watch?v=5NV6Rdv1a3I[/URL]"),
        add_link("https://www.youtube.com/watch?v=5NV6Rdv1a3I")
    );
}

#[test]
fn url_tag_is_case_insensitive() {
    assert_eq!(
        ok("!song add [url]https://youtu.be/5NV6Rdv1a3I[/Url]"),
        add_link("https://youtu.be/5NV6Rdv1a3I")
    );
}

#[test]
fn url_tag_with_value_gives_the_link_not_the_text() {
    assert_eq!(
        ok("!song add [URL=https://youtu.be/5NV6Rdv1a3I]my favourite song[/URL]"),
        add_link("https://youtu.be/5NV6Rdv1a3I")
    );
}

#[test]
fn url_tag_works_after_flags() {
    assert_eq!(
        ok("!song add --source youtube [URL]https://youtu.be/5NV6Rdv1a3I[/URL]"),
        Command::Add {
            query: QueryType::Link(Url::parse("https://youtu.be/5NV6Rdv1a3I").unwrap()),
            source: Some(SourceKind::Youtube),
            kind: SearchKind::Track,
        }
    );
}

#[test]
fn id_keeps_its_case_inside_url_tag() {
    assert_eq!(
        ok("!song add [URL]https://youtu.be/AbCdEfGhIjK[/URL]"),
        add_link("https://youtu.be/AbCdEfGhIjK")
    );
}

#[test]
fn half_a_tag_is_left_alone() {
    assert_eq!(
        ok("!song add [URL]lofi"),
        add("[URL]lofi", None, SearchKind::Track)
    );
    assert_eq!(
        ok("!song add lofi[/URL]"),
        add("lofi[/URL]", None, SearchKind::Track)
    );
    assert_eq!(
        ok("!song add [URL=lofi[/URL]"),
        add("[URL=lofi[/URL]", None, SearchKind::Track)
    );
}

#[test]
fn tag_in_the_middle_of_a_phrase_is_left_alone() {
    assert_eq!(
        ok("!song add songs like [URL]x[/URL] please"),
        add("songs like [URL]x[/URL] please", None, SearchKind::Track)
    );
}

#[test]
fn multi_byte_text_near_the_tag_does_not_panic() {
    assert_eq!(ok("!song add żółć"), add("żółć", None, SearchKind::Track));
    assert_eq!(
        ok("!song add ąę[/URL"),
        add("ąę[/URL", None, SearchKind::Track)
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
fn empty_url_tag_is_missing_query() {
    let usage = CommandError::Usage(
        "!song add [--track|--album|--playlist] [--source <name>] <link | phrase>",
    );
    assert_eq!(err("!song add [URL][/URL]"), usage);
    assert_eq!(err("!song add [URL=]text[/URL]"), usage);
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

#[test]
fn typed_words_in_errors_cannot_format_the_chat() {
    assert_eq!(
        err("!song add --x[b] y").to_string(),
        "unknown flag --x\\[b\\], try: --source, --track, --album, --playlist"
    );
    assert_eq!(
        err("!song add --source [url]x y").to_string(),
        "unknown source \\[url\\]x, try: tidal, spotify, youtube, soundcloud, local"
    );
    assert_eq!(
        err("!queue remove [b]").to_string(),
        "\\[b\\] is not a positive number"
    );
}
