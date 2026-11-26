//! Text for the TeamSpeak chat.
//!
//! The chat renders BBCode (`[b]`, `[url=...]`) in TS3 and TS6, and TS6 also renders Markdown.
//! Text from outside the bot (YouTube titles, words the user typed) goes through
//! `escape` before it is put into a reply, so it can never format or link anything.

use std::borrow::Cow;

/// Placed after a backslash that stands right before a bracket, see `escape`.
const ZERO_WIDTH_SPACE: char = '\u{200B}';

/// The longest message TeamSpeak takes. Counted in bytes, which are never fewer than
/// characters, so it is safe whichever of the two the server counts.
pub const MAX_MESSAGE_LEN: usize = 1024;

/// Marks text that was cut.
const MORE: &str = "…";

/// Cuts `text` to `max_chars` characters, the last one becomes `MORE`.
/// Cut before `escape`, so an escaping backslash is never separated from its bracket.
pub fn shorten(text: &str, max_chars: usize) -> Cow<'_, str> {
    if text.chars().count() <= max_chars {
        return Cow::Borrowed(text);
    }
    let kept: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    Cow::Owned(format!("{}{MORE}", kept.trim_end()))
}

/// Joins `lines` into one message of at most `MAX_MESSAGE_LEN` bytes, with `tail` always
/// as its last line. Lines that do not fit are left out from the end and `MORE` stands for them.
pub fn message(lines: impl IntoIterator<Item = String>, tail: Option<String>) -> String {
    let reserve = tail.as_ref().map_or(0, |tail| tail.len() + 1) + MORE.len() + 1;
    let mut text = String::new();
    let mut cut = false;
    for line in lines {
        let separator = usize::from(!text.is_empty());
        if text.len() + separator + line.len() + reserve > MAX_MESSAGE_LEN {
            cut = true;
            break;
        }
        if separator == 1 {
            text.push('\n');
        }
        text.push_str(&line);
    }
    for last in cut.then(|| MORE.to_owned()).into_iter().chain(tail) {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&last);
    }
    text
}

/// Makes `[` and `]` plain text by putting `\` before them, checked by hand in TS3 and TS6:
/// the backslash is not shown, tags (also unclosed ones) and Markdown links `[text](url)`
/// stay plain text. Markdown emphasis (`**`, `_`) is left alone: it cannot link anywhere,
/// and a backslash before it shows up in TS3.
///
/// A backslash already right before a bracket (`x\[b]`) gets an invisible space after it,
/// so it cannot cancel the escaping backslash. Text without brackets is returned as it is.
pub fn escape(text: &str) -> Cow<'_, str> {
    if !text.contains(['[', ']']) {
        return Cow::Borrowed(text);
    }
    let mut escaped = String::with_capacity(text.len() + 8);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '[' | ']' => {
                escaped.push('\\');
                escaped.push(c);
            }
            '\\' if matches!(chars.peek(), Some('[' | ']')) => {
                escaped.push(c);
                escaped.push(ZERO_WIDTH_SPACE);
            }
            _ => escaped.push(c),
        }
    }
    Cow::Owned(escaped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_not_cut() {
        assert_eq!(shorten("Waterloo", 8), "Waterloo");
        assert!(matches!(shorten("Waterloo", 8), Cow::Borrowed(_)));
    }

    #[test]
    fn long_text_is_cut_to_the_limit_with_an_ellipsis() {
        assert_eq!(shorten("Dancing Queen", 8), "Dancing…");
        assert_eq!(shorten("Zażółć gęślą jaźń", 6), "Zażół…");
        assert_eq!(shorten("ab cd", 4), "ab…");
    }

    #[test]
    fn message_keeps_lines_that_fit() {
        let lines = vec!["one".to_owned(), "two".to_owned()];
        assert_eq!(
            message(lines, Some("page 1/2".into())),
            "one\ntwo\npage 1/2"
        );
        assert_eq!(message(Vec::new(), None), "");
    }

    #[test]
    fn message_cuts_lines_from_the_end_and_keeps_the_tail() {
        let lines = (0..100).map(|n| format!("{n:02}. {}", "x".repeat(40)));
        let text = message(lines, Some("page 1/9".into()));
        assert!(text.len() <= MAX_MESSAGE_LEN, "{}", text.len());
        assert!(text.starts_with("00. "));
        assert!(text.ends_with("\n…\npage 1/9"), "{text}");
    }

    #[test]
    fn text_without_brackets_is_borrowed() {
        for text in ["Daft Punk - Get Lucky", "", "AC\\DC **bold** :/", "zolc"] {
            assert!(matches!(escape(text), Cow::Borrowed(_)), "{text}");
            assert_eq!(escape(text), text);
        }
    }

    #[test]
    fn brackets_get_a_backslash() {
        let cases = [
            ("[b]x[/b]", r"\[b\]x\[/b\]"),
            (
                "[url=https://evil.example]Lyrics[/url]",
                r"\[url=https://evil.example\]Lyrics\[/url\]",
            ),
            (
                "[link](https://evil.example)",
                r"\[link\](https://evil.example)",
            ),
            ("Song [Official Video]", r"Song \[Official Video\]"),
            ("unclosed [b", r"unclosed \[b"),
            ("]", r"\]"),
        ];
        for (text, expected) in cases {
            assert_eq!(escape(text), expected, "{text}");
        }
    }

    #[test]
    fn backslash_before_a_bracket_cannot_cancel_the_escape() {
        assert_eq!(escape(r"x\[b]"), "x\\\u{200B}\\[b\\]");
        assert_eq!(escape(r"\\[b]"), "\\\\\u{200B}\\[b\\]");
    }

    #[test]
    fn other_backslashes_stay() {
        assert_eq!(escape(r"C:\music\[x]"), "C:\\music\\\u{200B}\\[x\\]");
        assert_eq!(escape(r"a\b [c]"), r"a\b \[c\]");
    }

    #[test]
    fn multi_byte_text_is_kept() {
        assert_eq!(escape("Kult [żółć] 🎵"), r"Kult \[żółć\] 🎵");
    }

    #[test]
    fn every_bracket_is_escaped_and_nothing_else_changes() {
        let text = "a[b]c[[d]]e\\f";
        let escaped = escape(text);
        let unescaped = escaped
            .replace(ZERO_WIDTH_SPACE, "")
            .replace("\\[", "[")
            .replace("\\]", "]");
        assert_eq!(unescaped, text);
        assert_eq!(escaped.matches("\\[").count(), text.matches('[').count());
        assert_eq!(escaped.matches("\\]").count(), text.matches(']').count());
    }
}
