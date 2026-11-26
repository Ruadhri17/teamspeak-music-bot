use std::borrow::Cow;

use crate::template::{Placeholder, Template, UnknownPlaceholder};

fn fill(text: &str) -> String {
    let template: Template = text.parse().unwrap();
    template.render(|placeholder| match placeholder {
        Placeholder::Song => Cow::Borrowed("Get Lucky"),
        Placeholder::Source => Cow::Borrowed("YouTube"),
        Placeholder::Default => Cow::Borrowed("Music Bot"),
    })
}

#[test]
fn placeholders_are_filled_in() {
    assert_eq!(
        fill("Now playing: {song} from {source}"),
        "Now playing: Get Lucky from YouTube"
    );
    assert_eq!(fill("{default} | {song}"), "Music Bot | Get Lucky");
    assert_eq!(fill("{song}{song}"), "Get LuckyGet Lucky");
}

#[test]
fn text_without_placeholders_stays_as_it_is() {
    assert_eq!(fill("Hello!"), "Hello!");
    assert_eq!(fill(""), "");
    let template: Template = "Hello!".parse().unwrap();
    assert_eq!(template.placeholders().count(), 0);
}

#[test]
fn other_braces_are_plain_text() {
    for text in [
        "{ song }", "{Song}", "{}", "{song", "song}", "[b]{[/b]", "{{song}",
    ] {
        let template: Template = text.parse().unwrap();
        let expected = text.replace("{song}", "Get Lucky");
        assert_eq!(fill(text), expected, "{text}");
        assert_eq!(template.as_str(), text);
    }
}

#[test]
fn unknown_placeholder_is_an_error() {
    let err = "Now playing: {snog}".parse::<Template>().unwrap_err();
    assert_eq!(err, UnknownPlaceholder("snog".into()));
    assert_eq!(
        err.to_string(),
        "unknown placeholder {snog}, the known ones are {song}, {source} and {default}"
    );
    assert!("{artist} - {song}".parse::<Template>().is_err());
}

#[test]
fn placeholders_are_listed_in_order() {
    let template: Template = "{default}: {song} ({source}), {song}".parse().unwrap();
    assert_eq!(
        template.placeholders().collect::<Vec<_>>(),
        [
            Placeholder::Default,
            Placeholder::Song,
            Placeholder::Source,
            Placeholder::Song
        ]
    );
}

#[test]
fn template_is_written_back_as_given() {
    let template: Template = "Now playing: {song}".parse().unwrap();
    assert_eq!(String::from(template), "Now playing: {song}");
}
