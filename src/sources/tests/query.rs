use url::Url;

use crate::sources::{QueryType, parse_query};

fn link(text: &str) -> QueryType {
    QueryType::Link(Url::parse(text).unwrap())
}

fn phrase(text: &str) -> QueryType {
    QueryType::Phrase(text.to_owned())
}

#[test]
fn text_with_spaces_is_a_phrase() {
    assert_eq!(parse_query("get lucky"), phrase("get lucky"));
    assert_eq!(parse_query("daft\tpunk"), phrase("daft\tpunk"));
}

#[test]
fn text_with_spaces_is_a_phrase_even_if_it_starts_with_a_link() {
    let text = "https://youtu.be/5NV6Rdv1a3I please";
    assert_eq!(parse_query(text), phrase(text));
}

#[test]
fn single_word_is_a_phrase() {
    assert_eq!(parse_query("Darude"), phrase("Darude"));
}

#[test]
fn http_and_https_links_are_links() {
    for text in [
        "https://www.youtube.com/watch?v=5NV6Rdv1a3I",
        "http://youtu.be/5NV6Rdv1a3I",
        "HTTPS://YOUTU.BE/5NV6Rdv1a3I",
    ] {
        assert_eq!(parse_query(text), link(text), "{text}");
    }
}

#[test]
fn link_with_unknown_host_stays_a_link() {
    assert_eq!(
        parse_query("https://evil.com/x"),
        link("https://evil.com/x")
    );
}

#[test]
fn other_schemes_are_phrases() {
    for text in [
        "abba:waterloo",
        "localhost:8080",
        "ftp://youtube.com/watch?v=5NV6Rdv1a3I",
        "file:///etc/passwd",
        "javascript:alert(1)",
    ] {
        assert_eq!(parse_query(text), phrase(text), "{text}");
    }
}

#[test]
fn schemeless_link_to_known_host_gets_https() {
    let cases = [
        ("youtu.be/5NV6Rdv1a3I", "https://youtu.be/5NV6Rdv1a3I"),
        ("YOUTU.BE/5NV6Rdv1a3I", "https://youtu.be/5NV6Rdv1a3I"),
        (
            "www.youtube.com/watch?v=5NV6Rdv1a3I",
            "https://www.youtube.com/watch?v=5NV6Rdv1a3I",
        ),
        (
            "music.youtube.com/watch?v=5NV6Rdv1a3I",
            "https://music.youtube.com/watch?v=5NV6Rdv1a3I",
        ),
    ];
    for (text, expected) in cases {
        assert_eq!(parse_query(text), link(expected), "{text}");
    }
}

#[test]
fn schemeless_text_with_unknown_host_is_a_phrase() {
    for text in [
        "Mr.Brightside",
        "evil.com/watch?v=5NV6Rdv1a3I",
        "youtube.com.evil.net/watch?v=5NV6Rdv1a3I",
        "127.0.0.1/song.mp3",
    ] {
        assert_eq!(parse_query(text), phrase(text), "{text}");
    }
}

#[test]
fn id_keeps_its_case_in_a_schemeless_link() {
    let QueryType::Link(url) = parse_query("YOUTU.BE/AbCdEfGhIjK") else {
        panic!("expected a link");
    };
    assert_eq!(url.path(), "/AbCdEfGhIjK");
}

#[test]
fn empty_text_is_an_empty_phrase() {
    assert_eq!(parse_query(""), phrase(""));
}
